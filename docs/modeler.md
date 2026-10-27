# The 3D modeler (`kadakas-modeler`)

`kadakas-modeler` is a second UI flavor of Kadakas (PLAT-11). It is a separate program. It has a large 3D view and the layout of familiar 3D modeling programs. It opens and saves the same `.kad` files as `kadakas-app`. This page describes MOD-01 (the program) and MOD-02 (navigation). The rows are in section 4.23 of `docs/feature-spec.md`.

## Start it

- The launcher lists it as a UI flavor: **Kadakas Modeler**. The launcher finds the program `kadakas-modeler` in the version folder. It needs no `flavors.json`. The installers and the update archives bring the program.
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
| Tray | **Entity Info**, **Tags**, **Materials**, **Soften Edges**, **Scenes** and **Styles**. A click on a title bar collapses the panel. View > Tray hides the tray. |

Select, the drawing tools of MOD-08, Move, Rotate, Scale, the Paint Bucket, Tape Measure and the navigation tools run.

**Measurements box.** It shows the size of the selection (length, width and height in the units of the person), the field of view while the Zoom tool is active, and the live value of a drawing tool that runs (length, width and height, radius, bulge, distance). You type into it without a click: digits and the marks of values start the text, letters follow once the text has started (`24s`, `500r`, `3'6"`), Backspace erases, Enter applies and Esc clears. See "Modeling" below.

**Entity Info.** It shows the type, the id, the layer, the class, the size and the lowest corner of the selected object. For several objects it shows the count and the size of the box around them. With no selection it shows the active layer and class. A surface model also shows its numbers of vertices, edges and faces and whether the faces close a volume. A selected face shows its area, normal, edges and holes. After a merge (Erase, or a flush Push/Pull), the face that stays has the lower id and its own attributes and material. It also has the attributes that it did not have from the merged faces. The status line names what was taken or dropped. A selected edge shows its length, ends, curve and the number of faces on it. Several faces and edges show their counts. **Materials** is the list of the materials of the model (see "Paint Bucket and Materials" below). **Scenes** and **Styles** are described in "Scenes and styles" below.

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

## Modeling (MOD-08)

Move, Rotate and Scale are in the section after this one.

The code that decides is in `kadakas_ui_core::modeling` (no window, no GPU, no clock). The modeler only feeds it the pointer and the keys, runs what it returns and draws. `crates/kadakas-modeler/src/edit3d.rs` is the glue.

### Look and selection

- A surface model is drawn by the scene builder of `kadakas-core`: the faces as triangles and every edge (not the ones with the attribute `hidden`) as a line. A face shows its **front** in a light color and its **back** in a bluish color (a flag on the vertices of the mesh, `Mesh::two_sided`; the GPU, the CPU renderer and so the exports all draw the back in the back color `SURFACE_BACK`. A face with a material shows the material on both sides.) Edges with the attribute `soft` (the sides of a cylinder) are not drawn as lines. The faces on the two sides of an edge with the attribute `smooth` shade as one surface (their vertex normals are shared); every other edge is a crease, also between faces at a small angle. The GPU view and the CPU renderers draw the same scene, so they shade the same.
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
| Polygon (no key) | center, then a corner; Ctrl (Option on a Mac) switches to a polygon around the circle | a radius; `8s` for the number of sides (the default is 6) |
| Offset (F) | click a face (or anywhere with one face, or a chain of edges, selected), move, click | a distance; a negative one goes to the other side |
| Follow Me (no key) | with the path selected, click the face; else click the face, then the path (an edge or a face) | nothing |

A circle or an arc is made by `edit_surface` with the action `add_curve`: a chain of edges that stays **one curve**, also after other edges cut it. The Eraser removes the whole curve. Pushing or pulling a circle copies the curve to the cap, and the new edges between the pieces get the attribute `smooth`.

**Typing after a tool ended.** Right after a Rectangle, Circle, Polygon, Arc, Push/Pull or Offset a new value redoes it: the app takes back that step and makes the new one (still one undo step). This only works while nothing else changed the document.

**Push/Pull in the core** (`edit_surface` with `push_pull {face, distance}`) decides for each edge of the face. An edge shared with one wall at a right angle is stretched: the wall follows the edge (and gets a step where the face is split from a neighbor). "At a right angle" is a distance test: the moved edge must stay within the plane tolerance of the wall, so a wall that leans a little stretches for a short push and gets a new wall beside it for a long one. Any other edge gets a new wall. A lone face becomes a closed solid; a face on other faces (a square drawn on a wall, the half of a split top) becomes a bump, or a pocket for a push, and the opening stays. The face keeps its id and is the cap. When the new vertical edge would lie on an edge of the model (a top split from corner to corner and pushed down, so the vertical runs down the corner of the box), the two merge as in SketchUp: the corner edge is split at the new floor. A stretched edge that is replaced keeps its attributes and its place in a curve. **Flush:** when the face lands on the geometry along its normal (a bump pushed back down, a pocket floor pulled back up to the top, the second half of a split top pushed to the level of the first), the walls between collapse, the vertices that meet are welded, edges that fall together become one (the older id stays; edge attributes that the weld drops give a note), and the face merges with the face of the same plane that it now touches, by the rules of Erase (the face with the lower id stays, so a pocket pulled back up gives the original top again). A cap vertex (or the copy of a vertex that stays) also welds when it lands on a vertex or inside an edge of a face in the plane of the cap. The edge is split there first (a welded T-junction). Example: a square at the foot of a step, pulled up to the high side, merges with the high side, and the part of the step wall beside it goes. A wall whose middle collapses is cut into the parts that keep an area. A push that lands within the length tolerance (0.01 mm) of such geometry goes exactly there, so the cap stays in one plane. A push that ends farther away leaves a thin wall (a wall of 0.02 mm is valid). Which ids stay: the vertex that was there stays, and the moving vertex or copy goes. Of two edges that fall together, the lower id stays. Of two faces that merge, the lower id stays. A circle that lands on its own ring gives its place in a curve to the ring (an edge is never in two curves). The ring then goes with the edges that the merge erases. **Attributes of a merge** (Erase and a flush): the id that stays is the lowest. The look is that of the merged face with the largest area (a tie goes to the lower id): its attributes, a material too, become those of the new face, whole. Nothing is taken from the smaller faces. So a small face never changes how a large one looks: a bump with a material, pushed flat, does not paint the top, and the material of the bump is dropped. Each value of a smaller face that the larger face does not have, or has with another value, is dropped with a note in the result of the command (`notes`, in the JSON of `execute`). The modeler shows the note in the status line. So the edit never drops a material silently. It is close to the rule for faces that land on one face when models merge (MOD-07), where the larger area also decides. SketchUp has no documented rule for this (forum posts disagree), so this one is our own choice. A face that collapses with attributes, and an edge attribute that a weld drops, give a note too. A push that passes a vertex along the normal (the top pushed past a line across a wall) goes in steps: to the line, then on. A face that merged on the way stops there, and a push past it is refused ("a push past that is a later step"). This is by design. Example: a pocket floor with a bump on it, pulled to the top, is refused at the height of the bump. The person pulls to the bump (the two faces merge), then on to the top. The distance you type is in world mm; the tool turns it into the units of a scaled model. **Hole through.** A face drawn on one side of a slab (a rectangle on the top of a box, the outline of a pool or a pit) and pushed exactly to the other side cuts a hole through: the cap goes, the face of the other side gets a hole of the same shape, and the walls of the pocket are the walls of the hole. The result is closed when the slab was. A distance within the length tolerance of the other side goes exactly there. A ring pushed through leaves a column with an island face at the bottom. The walls keep the paint of the face, a face with a material gives a note that its cap went, and curves and soft edges stay (a round hole has round walls). It is one undo step. The status line gives the reasons of these edits in the language of the window, without the internal ids (the core keeps the English text with ids for agents and logs). Refused with a reason: a second solid that stands under the slab, back to back with its bottom under the hole ("another solid stands under the slab there"), a face whose cap reaches the other side but is not fully inside one face there (it touches or crosses its border or an old hole, or lies across two faces of the other side: erase the edge between them first), and a push past the other side (it cuts through other faces). **Refused:** a push that reaches the opposite face in another way (the face would become flush with a face that looks the other way, beside it or under it, a slab pushed to zero thickness, or a copy would end on it); a push into a wall that shares an edge of the face with another face (a fin: the new wall would lie on that wall); and any result in which new or moved geometry passes through, ends on or lies in another face (also a stretched wall that would cut a plate), or that has a model problem the model did not have before (the error names the new problem). A contact or a problem counts as old only when the model had one of the same kind at the same place, so an old contact on a split-off part counts as old, and an old problem that moved counts as new. These checks run on the touched geometry also when the model had a problem before. The error says why and nothing changes. The model check (`check`) now also reports an edge that passes through a face, an edge of a face that lies inside a coplanar face, and two coplanar faces that overlap (also in part; faces that only touch along an edge or at a vertex do not). The sides count. Two faces of one plane that share an area and look the same way overlap. That is an error (also in the fast check, at a shared edge). Two faces that look opposite ways lie back to back (a box that stands on a slab, with its own vertices). That is valid. It goes to the warnings of the report (`BackToBack`), and the solid tools (MOD-09) can use it as an inner face. A hole of a face is not an area of the face. A face inside the hole overlaps nothing. A face that covers the hole and the material around it overlaps, and its edges show it. A face is tested through an index of its sides, so a face with thousands of holes costs `O(log n)` per test. On a model of 50 000 edges a push takes about 150 to 650 ms and a full check about 250 to 400 ms (release build; the high end is a slab top with 4,200 bumps).

### Polygon, Offset, Follow Me and soft edges (MOD-08, second part)

**Polygon.** A regular polygon with 6 sides by default (`8s` before, during or just after the tool sets the number, from 3 to 1,000, as for a circle). The radius goes to a corner, as in other modelers of this kind (inscribed); with Ctrl (Option on a Mac) it goes to the middle of a side (circumscribed). The polygon is drawn with `add_curve` and `faceted: true`, so it is one curve like a circle (the Eraser takes it whole, the inference offers its center, and it stays one curve when other edges cut it), but its edges carry the mark `faceted`: the edges that Push/Pull, Follow Me and Offset make between its sides stay creases, drawn and hard, because the corners of a polygon are real corners. A pulled hexagon is a prism with six visible side lines; a pulled circle is a cylinder with soft and smooth sides. The copies of a faceted curve are faceted too.

**Offset** (`edit_surface` with `offset_face {face, distance, outward}`): every loop of the face is drawn again `distance` mm inside the face (or outside with `outward`), in its plane. Neighbor sides meet at the crossing of their moved lines (a miter, no rounded corners), so a concave corner and a hole work too (a hole grows with an inward offset and shrinks with an outward one). The new loops are drawn as `add_edges` draws: an inward offset splits the face into a ring along each loop and the face inside (the pieces keep the paint), an outward one splits the coplanar face around it or makes a ring face in the empty plane around it. Faces in other planes are not cut. A run of sides of one curve gives one curve again, so the offset of a circle is a circle. Refused with a reason and nothing changed: a distance that is not larger than the length tolerance (the direction is `outward`, never a negative distance), and an offset that does not fit (a side would turn around or vanish, a new loop would cross itself or another, a hole would close or leave the face); the message gives the largest distance that fits ("at most about 499.99 mm fits"), found by halving. **Offset of edges** (`offset_edges {edges, distance, toward}`): a chain of edges in one plane (open or closed, no branches) is drawn again to the side of the point `toward`; the ends of an open chain move at a right angle. A straight chain lies in the plane of the face it bounds that holds the point `toward` (a line on the top edge of a box offsets into the top or the front, wherever the pointer is), else in the horizontal plane. The tool: Offset with a chain of edges selected; the pointer goes into the plane of the chain (a snapped point counts as it is), and the distance is in world mm also in a scaled model; a negative typed value goes to the other side, also while the tool runs. With several faces selected, Offset works on the one under the pointer and says so when the pointer is on none of them. The largest distance that fits is found by halving down to a sixteenth of the length tolerance (about 24 steps for 10 m), and holes are tested against each other through a box tree. Measured (debug build): an inward offset of a face with 900 holes takes 0.19 s; a refused one (5 m on that face, "at most about 499.99 mm fits") 0.05 s.

**Follow Me** (`follow_me {face, path}`): the face (the profile) is swept along the chain of path edges. The profile moves along each side of the path to the plane at the next corner, which halves the angle between the two sides (a miter); the plane at the end of an open path is at a right angle to the last side. Each point moves along a side of the path, so each new face is flat by construction and a profile at any angle to the path works (it turns with the path at each corner; a flat path keeps the distances to it). This is our own definition, chosen because it needs no frame and keeps every face planar. An **open path** gives a solid with caps: the profile stays in its place as the first cap (turned to look away from the sweep) and a new face is the end cap. A **closed path** gives a ring without caps: the sweep starts at the side of the path nearest to the middle of the profile, at its end nearer to the profile (so the order in which the edges were drawn does not matter), the profile goes, and a note says so when the profile was moved to that corner; a path that does not bring the profile back to its start is refused (a flat path does). A point of the profile on the axis of a turn stays one vertex, so a half section on a circle turns into a solid of revolution (a lathe). New faces side by side in one plane (the top of an L, the disc of a lathe) become one face. The new faces get the paint of the profile; where two sides of the path are one curve the copy of the profile is soft and smooth, and where two sides of the profile are one curve the edge along the path is soft and smooth, so a circle swept along a path shades round; the copies of a curve are curves. A sharp turn moves the points on the inside of the turn far: a turn of 175 degrees (a corner of 5 degrees) moves them about 23 times their distance from the path, and that is accepted as in other modelers of this kind. A point that would move backward (a profile too wide for the turn, or a lathe profile across the axis) would fold the solid: refused with the largest distance from the path that fits. Refused with a reason too: edges that are not one chain, a profile whose plane holds the first side of the path, a side of the profile along the path or one that sweeps into no area, a path that turns back, a face that fills a hole of the profile (erase it first), more faces than the model may have, more than 10,000 new edges (profile sides × path sides), and a result with a new model problem (a sweep that runs into other geometry, or a profile that touches the path inside one of its sides). A sweep that passes through itself is accepted with `NotIntersected` warnings. The tool: select the path first and click the face, or click the face and then an edge (its curve, else the chain of edges through it up to a branch) or a face (its outline). Measured (debug build): a circle of 48 sides along a path of 100 sides, 4,802 faces, in 0.1 s.

**Soft and smooth edges** (`set_edge_style {edges, soft, smooth}` and `soften_edges {edges, angle, smooth, coplanar}`; hidden is Hide, `set_hidden`). A **soft** edge is not drawn, not picked and not offered to the inference, like a hidden edge; with View > Hidden Geometry both are drawn ghosted and can be picked. The faces across a **smooth** edge shade as one surface. Exports draw the scene, so they leave soft edges out too. Unsoften never touches `hidden`, and Unhide never touches `soft`. The tools that make curved surfaces (Push/Pull of a curve, Follow Me along a curve, the folds of Move) make soft and smooth edges. A soft edge that is not smooth hides the line and keeps the crease in the shading (as in other modelers of this kind; this is why "Smooth normals" is a choice). A soft edge between coplanar faces does not merge them. The **Soften Edges** panel of the tray works on the selected edges and the edges of the selected faces: an edge between two faces at an angle below the slider (0° to 180°, default 20°) becomes soft (and smooth with "Smooth normals", on by default), one at or above it loses both flags; "Soften coplanar" softens edges between faces in one plane too. While the slider is dragged the view draws the edges it would soften, dashed, and nothing changes; when it is let go (or a box is clicked) the edit runs once: one edit, one undo step and one message to the other users. A change right after the last one, on the same selection, takes the last one back first, so a few tries stay one undo step. Measured (debug build) on a soft grid of 48,400 faces: the plan of the preview 0.12 s, the soft surfaces 0.15 s, the scene with the smooth shading 0.33 s.

**Soft surfaces.** A soft surface is a set of faces connected through soft edges (also two coplanar faces, after "Soften coplanar"); a smooth edge that is not soft and a hidden edge do not join faces. In the Select tool a click on a face of a soft surface selects the whole surface (its faces and the soft edges between them), Shift adds or takes out the whole surface, a double click selects the surface with all the edges of its faces, and a triple click selects all connected geometry as before; a face with no soft edge is selected alone. Window and crossing selections stay geometric. The tools that work on the face under the pointer (Push/Pull, Offset, the Paint Bucket without Ctrl or Shift, the profile of Follow Me, the Eraser) are not changed. Entity Info says "24 faces (soft surface)". The surfaces are found once per revision of the document, at the first click on the model. The rule follows the wording of the public help ("a soft edge is hidden and the faces it separates become one surface; selecting one face selects the whole surface"); it was not checked against the product. Open details: whether a single click or a double click selects the surface, and whether smooth-only edges should join.

**Files from development builds.** Before soft and smooth were apart, Push/Pull wrote only `smooth` on the sides of a cylinder and the drawing left smooth edges out. Such files now show the side lines of their cylinders until the edges are softened (nothing was released, so there is no migration). The context menu of the 3D view has Soften, Smooth and Unsoften for the selected edges. Entity Info shows the three flags of a selected edge.

### Circles and arcs with an exact description (MOD-15)

The Circle, Arc and Polygon tools give the curve its exact circle: center, normal, radius and, for an arc, the start and end angle.
The field is optional and sits next to the chain of edges. The number of segments is the number of edges. The core is `kadakas_core::surface::analytic`.

**Entity Info.** Select one edge of a curve, or all edges of it. The panel shows the shape (circle, arc or polygon), the true radius and the center in the drawing.
For an arc it also shows the sweep angle. It shows the circumference of a circle and the length of the chain for an arc or a polygon. A placement of the model bends a circle in some cases (a remap of the plan with a height scale bends a vertical circle and leaves a horizontal one). Entity Info tests the plane of the circle. If the placement keeps the two axes of that plane at one length and a right angle, the radius is true. Else the circle is an ellipse in the drawing, and Entity Info says so and shows no radius, sweep or length. A circle of a kind from a newer program shows its kind only.
The field Segments takes a number from 3 to 1,000.
The button Set segments rebuilds the chain on the same circle, in one undo step. The status line says what happened or why it was refused.

**Set segments** (`edit_surface` with `set_curve_segments {curve, segments}`). The ends of an arc and the first vertex of a circle stay.
The other vertices are new, at equal angles. The first edges keep their ids and their flags. New edges take the flags of the first edge.
Faces in the plane of the circle keep their loops with the new vertices. A loose circle, a disc, a ring, a D shape and an arc are supported.
The edit is refused, with a reason in the language of the window, in these cases.
- The curve has no exact circle.
- Another edge meets the chain inside it.
- A face on another plane uses the chain. A circle under the wall of a pushed cylinder is the usual case.
- The new edges are shorter than the length tolerance, or the result is an invalid model.

**What the other edits do.** The model keeps one rule: if a curve has the field, all its vertices lie on the circle within the length tolerance.
A vertex that another edge meets and that lies on a chord between two vertices on the circle is allowed. A vertex with only the two chain edges is never excused, so a fine circle does not follow its points off the circle. The rule is kept after every edit.

| Edit | The field |
|---|---|
| Move, rotate, mirror, uniform scale | follows the chain, with the new center and radius |
| Scale with unequal factors, move of single points | dropped, with a note. The chain stays a curve. A corner that slides along the circle keeps the field, and a polygon stays a polygon |
| Erase that cuts a circle | each piece is an arc on the same circle with its own angles |
| Erase at the end of an arc | the arc is shorter, the angles are updated, the note says so |
| Push/Pull, Follow Me, copy | the copy gets its own circle at the new place |
| Offset | the new curve gets the circle of its corners, with another radius |
| Group, component, explode, copy | fitted again where the chain lands. A group scaled unequally loses it, with a note |

The offset of a circle by a distance `d` has the radius `r - d / cos(pi / n)`, because the sides of the polygon move by `d`.
An arc offset has no field, because its ends move at right angles to the end edges and leave the circle.
An arc of one edge loses its field when it moves. A description of a kind from a newer program is read only and is dropped, with a note, by any change of its chain.
Heal and join keep the field of the curve that keeps its chain. Two curves are never joined into one.
The notes of an edit are told in the language of the window and show in the status line.

**Snap.** The center of a circle or an arc is an inference point (`Center`) from the field, also for an arc and for a chain that another edge cut.
The Vectorworks-like app offers it in its snap list too (`snap_geometry` of the plan).

**File.** The model field `curve_geometry` is a list of `[curve id, {description}]`. A model that has it is saved as version 2 (`v`). Other models stay version 1.
A program from before MOD-15 keeps a model of version 2 as it is and does not edit it. A file without the field loads as before.
The load never drops a row without a word. A row that does not fit its chain, or that belongs to a missing curve, is left out and the load notes say so.
A row of a kind that this version does not know is read only. It is kept as it is and written back while its chain is untouched.
A row that cannot be read at all is kept the same way. Any change of the chain drops it, with a note.
A damaged log entry, or one written by another version, is kept and not replayed. The load notes tell the two cases apart and name them on every load, until the entries leave the log of the last 64 edits.
The API is `mesh.curve_info` and `mesh.set_segments` (`docs/mesh-api.md`).

**Export.** DXF export keeps the mesh of a surface model. A true CIRCLE or ARC entity is a later step, as are STEP and IFC circles.

**Tests.** Unit tests for every edit in `surface/tests_analytic.rs`, a property test with random sequences of edits (the invariant holds, a twin model gives the same result), fixtures of an old file and of a file with unknown fields, the segment tests, the API tests, the tests of Entity Info and the window.

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

**Not done.** Glue To, Cut Opening, Reload and Save As, Always Face Camera, Live and Dynamic Components, Reset Scale and Skew, Change Axes, lock, a live update of the other instances while an edit is open, and a grey class on a part. The plan of an instance was the box of its definition (`component::plan_mark`, kept per definition and content version, read once per definition in a `plan_frame`). It is the faces and edges of its parts since MOD-13, and the box stays for bounds and picking.

## Solid tools (MOD-09)

The solid tools check that a part of a surface model is a closed solid. They join, cut and intersect two closed solids. The wording follows the public SketchUp help for Solid Tools. It is not verified against the product. No part of the product was used. The code is in `crates/kadakas-core/src/surface/solid.rs` and the modules below it.

### What a solid is

A solid is a set of faces that closes a volume. Each edge of the set has exactly two faces of the set. The two faces use the edge in opposite directions. The fronts of the faces look out. A model can hold many solids. Faces can hold a solid and a cavity. A cavity is a shell whose fronts look into a hollow. It belongs to the solid that holds it.

The core finds the faces of a solid with a walk. It starts at a face and goes over each edge to the face next to it in the material. At an edge with two faces this is the other face. At an edge with more faces it is the face that comes first when the face turns toward its back around the edge. A face that looks into that wedge is not taken. This is how a fin, or a second solid that shares the edge, is told apart. `SurfaceModel::solid_of(face)` gives the faces of the solid of a face.

### The check and the report of open edges

`SurfaceModel::solid_report()` is the closed-solid check. It runs the full check of the model once. It returns a `SolidReport` that serializes to JSON, so an agent can read it.

- `shells`: one entry for each shell, with its `faces`, its `state` (`solid`, `open`, `self_crossing`, `inverted` or `flat`), its `open_edges`, its `crossings`, its signed `volume` in mm³ and its `area` in mm².
- `open_edges`: each edge with a `kind`. `border` means one face uses it. `non_manifold` means three or more faces of the shell use it. `reversed` means two faces use it the same way round, so one of them needs Reverse Faces.
- `wire_edges`: edges that no face uses.
- `crossings`: the number of places where an edge passes through a face without an edge there. Two solids that cross are in this state. The solid tools cut them. The state is not held against either solid.

A shell is `flat` when it is closed but encloses no volume (two faces back to back). It is `inverted` when it is closed and its volume is negative. It is `self_crossing` when an edge of the shell passes through a face of the same shell. `SolidReport::is_closed()` is true when every shell is a solid and no edge is loose. `SurfaceModel::solid_state(faces)` and `solid_volume(faces)` give the same for a list of faces.

Check Solid is in the Edit menu, under Solid Tools. It works on the selected faces and edges, else on the selected surface models. The status line says how many solids are closed and their volume. When something is open, it says how many edges are open, and it selects those edges (and the loose edges) so that the person sees them.

### The operations

One edit does one operation: `edit_surface` with `{"action": "solid", "op": "union", "first": [face ids], "second": [face ids]}`. The lists are the faces of the two solids. Both solids must be in the model of the command. It is one undo step. The `op` is one of:

- `union`: one solid that holds both.
- `subtract`: `first` less `second`. `second` is used up.
- `intersect`: only the volume that both hold. Both are used up.
- `trim`: like subtract. `second` stays in the model, but it is split along the cut and stuck to `first`. The two share the vertices and edges of the cut, and the cut faces lie back to back.
- `split`: three solids in place: `first` less `second`, the common part and `second` less `first`. They touch with faces back to back and are stuck to each other along the cut, as in trim. A part that would be empty is left out.

How it works:

1. Both lists must be solids (state `solid`). They must not share an edge or a vertex with other geometry of the model. Nothing is changed before this is known.
2. A copy of the model that holds only the faces of the two solids is cut with Intersect Faces. Faces of one solid that the other crosses are split along the crossing. A crossing that is too small to cut is a refusal.
3. Each piece is wholly inside, outside or on the surface of the other solid. A ray cast against the triangles of the other solid tells which. A point on the surface (within the plane tolerance) is told by the direction of the face, the same way or the opposite way. If the points of one piece disagree, the piece was not cut and the operation is refused.
4. The rules of the operation keep, drop or turn over each piece. A face that both solids use is on the surface of both, looking the same way. Two solids that share a plane in one model share the faces there.
5. Pieces in one plane with the same direction and the same paint merge again, but only along seams that the cut made. A line or a circle that the person drew is never merged away, also where the cut crosses it. Edges and vertices that no kept face uses go. A curve that loses edges stays as the chains that are left (the rule of MOD-06). Soft and smooth edges and paint stay with their pieces.
6. The solids of the result must be closed. The common volume `I` is the volume of the surface of the pieces of the first solid that are in or on the second, and the pieces of the second that are in the first. It is found before the rules of the operation are applied, so it is a check that does not depend on them. `I` must be between 0 and the smaller input. The result must have exactly these volumes: union `va + vb - I`, subtract and the cut solid of trim `va - I`, intersect `I`, the kept solid of trim `vb`, and split `va - I`, `I` and `vb - I` (to 1 part in 1,000,000). The model must have no problem that it did not have before.
7. The result replaces the geometry of the two solids.

The edit is deterministic. The same model and the same edit give the same ids. The edit cannot be replayed on another model for collaboration (the solids are sets of faces), so a concurrent edit by another person shows as a conflict.

### Refusals

The edit changes nothing and gives a reason. The core text is English with ids, for agents and logs. The window shows the same reason in en-US or Estonian without ids.

- an input has no faces, or a face does not exist
- an input is not a closed solid (the text names the number of open edges and one edge)
- an input passes through itself
- an input looks inward (use Reverse Faces) or has no volume
- an input is stuck to other geometry (shares an edge or a vertex): make a group of it first
- the result would be empty (no common volume for intersect, or the second solid holds the first for subtract)
- the cut is too small, or a piece is cut only in part
- a vertex of one solid lies off a face plane of the other by more than the plane tolerance (0.0025 mm) but not more than the length tolerance (0.01 mm). The piece is too short to draw and the vertex is too far off to count as on the face. The text names the distance. Move one solid by at least 0.01 mm along the face normal, or set the vertex exactly on the face
- the result is not a closed solid (for example two solids that touch at an edge, which would leave an edge with four faces), its volumes do not fit the volumes of the inputs and `I`, or it would give the model a problem that it did not have
- a limit would be passed: a cut makes at most `MAX_EDIT_EDGES` (10,000) new edges, and the result must fit the limits of the model

**Known limit.** After split and trim the pieces share vertices and edges with the other pieces. A later solid operation on one of them is refused as stuck to other geometry. Make Group on a piece gives it a model of its own. The solid tools do not yet work across models, so this is only a way to keep the piece apart. A tool that unsticks the pieces is not done.

Two boxes that touch with a shared face, built one on the other in one model, are one complex that no solid owns. The shared face cannot look out of both. The tools refuse it (the reason is an edge that two faces use the same way round). To join such boxes, erase the shared face.

### Modeler

Edit > Solid Tools holds Check Solid, Union, Subtract, Intersect, Trim, Split and Swap Order. An operation works on the selected faces of one model. The selection must name exactly two solids. The solid of each selected face is found by the walk. The older solid (the lower face id) is the first one. Swap Order makes the newer one the first. The items are disabled with the reason as a tooltip when the selection does not name two solids. An enabled item shows the preview as a tooltip: the volume of the result, or the reason the operation would be refused. The preview runs the operation on a copy, and only for the item under the pointer. It is kept for the revision, the selection and the order, so each of the five operations runs once on a copy. Running the item uses that preview and does not run the operation a second time. It shows the same text in the status line. A refusal shows in the status line and changes nothing. The window turns the typed refusal (`SolidError`) into its text, not the English text. The code is in `kadakas_ui_core::modeling::solid` and `kadakas-modeler/src/menu.rs`.

### Tests

- Unit tests of the core (`surface/tests_solid.rs`, `surface/solid/`): nine cases of two boxes (crossing, sharing planes, plus sign, nested, nested sharing a corner or a wall, disjoint, the same box twice, a post through a slab), the report for open, reversed, inward, flat and crossing shells, refusals, curved solids, a tube, two balls of 1,280 triangles, the limit of new edges, and the helpers of the ray cast.
- Property tests: for boxes on a grid (also Trim and Split) (planes, edges and corners shared often) and for a turned box, `vol(A union B) + vol(A intersect B) = vol(A) + vol(B)` and `vol(A subtract B) + vol(A intersect B) = vol(A)`. A refusal changes nothing.
- Integration tests (`crates/kadakas-core/tests/solid_tools.rs`): each operation is one undo step and comes back with undo and redo, a refusal makes no step, the edit and the report are JSON. Also lines and circles of the person that survive a union, a cylinder cut in half that keeps its curves as chains, and the count of previews in the window.
- Tests of the window and of `ui-core`: operands, order, preview in both languages, one undo step, refusals without ids, Check Solid, the menu.

## Scenes and styles (MOD-10)

A scene is a saved view of the document. It keeps the camera, which tags and layers are visible, the active tag and layer, and the render style. The scenes of the modeler are the saved views of the document, in the same order. The Vectorworks-like app lists the same saved views in its Navigation palette. One `.kad` file shows the same scenes in both programs. The wording of the panels follows the public SketchUp help for scenes and styles. It is not verified against the product.

### The Scenes panel

| Button | What it does |
|---|---|
| Add | Stores the present camera, tags, layers and style as a new scene. The name is `Scene 1`, `Scene 2` and so on. The name field opens for typing. |
| Update | Stores the present camera, tags, layers and style in the chosen scene. The scene keeps its place in the list. |
| Rename | Changes the name. A double click on a scene does the same. The name must be free (case does not count) and not empty. |
| Delete | Deletes the chosen scene. Undo brings it back. |
| Up and Down | Move the chosen scene in the list. The list is the order of the scenes. |

A click on a scene shows it. The tags and layers are set to the saved state, the active tag and layer are restored, the render style is set, and the camera moves to the saved view. This is one undo step for the document. The camera is not part of the undo history, like every camera move. A star after the name of the scene that was shown last means that the camera, a tag, a layer or the style changed since. Update stores the change. A tooltip says so.

The camera moves in a short animation of half a second (smoothstep easing, the short way round for the turn, even steps for the distance). A change of projection happens at the start. **View > Animate Scene Changes**, and the check box at the bottom of the panel, switch the animation off. The choice is kept in `modeler.json` (`animate_scenes`). The animation stops when the person moves the view. Entity Info is not changed by scenes.

### The Styles panel

The panel has a list of styles. It offers the five built-in styles from 3D-08 (Wireframe, Shaded, Realistic, Hidden Line, Dashed Hidden Line) and then the other render styles of the document. A built-in style joins the document as a render style resource when it is chosen for the first time. The 3D view draws with the render style of the document (the mode, edges, shadows, sun, textures and background). A choice is one undo step.

### What is stored

A scene is a `SavedView` of `drawing.saved_views`. MOD-10 added two optional fields. Older files have none. A program from before MOD-10 drops them when it saves.

- `camera`: the free orbit camera `{target [x, y, z], yaw, pitch, distance, fov, projection}`. Angles are radians. Lengths are mm. `fov` is degrees. The projection is `perspective` or `ortho`. The app has no free camera. It uses `view` and `projection`. The modeler stores the nearest standard view in `view`. The camera is checked: finite numbers, a distance above 0, a field of view from 1 to 170 degrees.
- `render_style`: the name of a render style. A restore finds it by name among the styles of the document, then among the built-in styles, which it adds. A name that does not exist any more (the style was deleted or renamed) leaves the style as it is and the rest of the view applies.

A scene also stores `mode`, the render mode of its style, because the app draws with the mode. A restore in the app takes the mode from the style when a view has no mode. A view with a mode and no style (a view of the app) gets the built-in style of that mode, unless the active style draws in that mode already. Unknown fields of a saved view are kept when the file is saved again. `saved_view.save` without `camera` keeps the camera of the scene with that name, and `camera: null` clears it.

A render style is found by name. A rename of a render style changes the name in the scenes. A delete of a style with another name than a built-in one clears the name in the scenes. A scene that names a style that does not exist shows "style ... is missing" in its row. A camera is valid with a yaw up to 1000 radians (a larger turn is wrapped when the modeler stores it), a pitch within 90 degrees and a distance of at least 0.001 mm.

A scene of the modeler has no `zoom` and no `pan`, which belong to the 2D plan. When the modeler shows a saved view that the app made, it uses the standard view and the projection of that view, and the plan view becomes the Top view. A saved view with no 3D part only sets the tags, layers and style, and the status line says so. When the app stores a view again under the name of a scene, it keeps the `camera` of that scene only if the view is still the nearest standard view of that camera. It also stores the active style.

### Commands and the API

Every action is one command and one undo step, so agents and the JSON-RPC API do the same. The existing methods were reused. These are new:

| Method | What it does |
|---|---|
| `saved_view.save` | New params `camera` and `render_style`. A scene with the same name is replaced in its place. Without `render_style` it stores the active style. |
| `saved_view.restore` | Also sets the render style. |
| `saved_view.rename` {name, new_name} | Command `RenameView`. |
| `saved_view.move` {name, to} | Command `MoveView`. 0 is first, a place past the end means last. |

The command `UseRenderStyle {name}` makes a style the active one and adds a built-in style to the document when needed (`execute`). The ID of an added style is in `created`. `render_style.*` works as before. The command schema (`crates/kadakas-core/tests/command_schema.json`) has the new commands.

The headless code is `kadakas_ui_core::scene_panel` (rows, commands, activation goal, styles, the camera transition) and the drawing is `kadakas-modeler/src/scenes_panel.rs`. The window runs the actions in `ModelerApp::scene_action`.

### Tests

- Unit tests of `scene_panel`: names, add, update, rename, delete and move with undo, the goal for a free camera and for a view of the app, the nearest standard view, the styles, the transition.
- Property tests: random scene edits keep the names unique and undo exactly, the saved camera survives JSON, the transition stays finite and between its ends.
- Tests of the window: each action is one undo step, refusals change nothing, the animation runs and can be switched off or interrupted, a saved file shows the same scenes when opened, a scene is a saved view for the API and the reverse, both languages.
- Core and API tests: the new commands, the restore of a built-in style, one undo step, and the JSON methods.

### Not done

- Scene thumbnails and scene tabs over the 3D view. The list in the tray is the only place.
- Per-scene choices of what to update (SketchUp lets a person pick which properties an update keeps). Update stores everything.
- Shadow settings, fog and sections of a scene. The sun belongs to the document.
- A scene transition in the Vectorworks-like app. It sets the standard view at once.
- Styles with edge and face settings of their own beyond the 3D-08 fields, and the style mixer.

The target of the camera of a scene is a point of the drawing in millimeters (drawing coordinates). It does not depend on the origin of the 3D scene, which moves when the visible drawing grows. A scene saved by an earlier build has its target relative to that origin and may look at another place after a change of the drawing. Save it again with Update.

## Agent API (MOD-11)

An agent drives the same geometry as the tools of the modeler. The methods are `mesh.*`. They work over JSON-RPC, over MCP (the tool `mesh`) and in Python (`doc.api.mesh_*`). Reads give the summary, vertices, edges and faces in pages, the solid report, bounds, faces near a point and a ray pick. Edits are thin wrappers over `edit_surface` and the group and component commands. Each edit is one undo step. `mesh.batch` runs up to 50 edits as one undo step, all or nothing. `mesh.soften_edges {below_angle}` softens every edge under an angle and says how many edges changed. `mesh.render` gives a picture of the result. A refusal has a stable `error_id`. The guide with examples is `docs/mesh-api.md`.

The edits of an agent are marked as agent changes and show in the Activity panel of the person. The modeler has no review window of its own for them.

## Import and export (MOD-12)

File > Import reads OBJ, STL, glTF, GLB, STEP and IFC. File > Export writes STL, OBJ, glTF, GLB, IFC and DXF. The windows build the calls of `import.file` and `export.file`, so the modeler and an agent use one code path. A mesh file becomes `mesh3d` objects. The window asks for the unit and the up axis of OBJ and STL files and for the move to the origin. It also asks "Soften edges under an angle?". The answer is off at first, with 20 degrees. When it is on, the window counts the edges that the angle would soften, and the import makes surface models that fit the limits. The answer is kept in `modeler.json`. For a STEP file the window asks for the unit (default: the file), the tolerance (default: from the size of the part), one mesh for each product and whether the exact surfaces are kept. Object Info of such a mesh says what it keeps. The guides are `docs/interop.md` and `docs/exact-solids.md`.

## Surface models in the Vectorworks-like app (MOD-13)

The Vectorworks-like app (`kadakas-app`) shows surface models, groups of them and component instances like other objects. It does not edit their geometry. The modeler does. Everything else works as for a line or a circle: layers, classes, visibility, grey layers, locks, Select, Move, Rotate, copy, delete and undo. The geometry code is one module, `kadakas_core::surface::plan`, so the screen, the sheets and the SVG show the same (AGR-01).

### The plan

The plan is the view from above.

- A face that looks up, or down, is drawn as its polygon with its holes. It is filled with the color of its material. A face without a material takes the fill of the object (or of its class). If there is none, the front is `SURFACE_FRONT` and the back is `SURFACE_BACK`, the same colors as in 3D. A face is seen from its back when its front looks down.
- A vertical face has no area in the plan and is left out. A vertical edge is a point and is left out.
- The faces are drawn from the lowest to the highest. Two faces whose footprints overlap are put in the order of their height at a point of the overlap, so a slope runs over a plate where it is above the plate and under it where it is below. A face is not cut, so where the heights cross inside the overlap, the point decides. That is a known limit. The whole plan of 20,000 faces costs 29 ms for a terrain and 41 ms for a ball, with the order in it (`docs/modeler-limits.md`, D1). A cycle is broken at the face that is lowest by its average height.
- Each face is followed by the edges that it is the highest face of. An edge that no face seen from above uses (a loose edge, the edge of a wall) is put in the order of its height: it comes before the first face whose average height is above it. So a roof over a wall covers the edges of the wall, and the top edge of a wall that stands out above the roof is drawn over the roof.
- A hidden face, a hidden edge and a soft edge are not drawn.
- The part of an instance whose class is switched off is not drawn, not shown in 3D and not snapped to. Which classes are off is the view of the thing that draws: the design view of the document on the screen, the classes of the viewport on a sheet, and for an export the option `visible_only` (a part on a class that is off is left out of the DXF with it, else it is written with the flag of an invisible entity).
- An instance that cannot be expanded (a missing or cyclic definition) is a cross in the warning color at its origin, with the reason in the SVG (`data-problem`).

A big model is simplified. The limits are `MAX_FACES` (20,000), `MAX_EDGES` (30,000) and `MAX_OUTLINE_EDGES` (20,000) of `surface::plan`. A model over the first two is drawn as its outline: the border, the silhouette (an edge where a face that looks up meets one that looks down or is vertical) and the loose edges, with no fills. If the outline is longer than 20,000 lines, the box is drawn. A triangulated mesh has 1.5 edges for each face, so the edge limit binds first: a terrain of 20,000 faces already has 30,200 edges. The numbers that these limits come from are in `docs/modeler-limits.md`, section D.

Nothing is simplified without a word. The same text is in every place that the person looks at:

- On the canvas: a note at the lower left corner of the object, like the notes of a pattern, for example "Drawn as outline: 34,000 faces, limit 20,000".
- In Object Info: the row Plan with the same text.
- In the SVG of the plan: `data-detail` (`full`, `outline` or `box`), `data-faces` and `data-edges`.
- On a sheet: `Page::notes`. The SVG writes them in `<desc>` and the PDF in the Subject of the document info, with the number of the page.
- In the API: `objects.get` and `objects.list` have `surface.plan` (`detail`, the counts, the limits and the `note`), and `render.svg` has `notes` and `simplified`. `render.png` in a hidden-line mode has `notes` when it left edges out.
- In a hidden-line viewport and in the edges beyond a section: a note of the page, "N object(s) with more than 50000 edges are drawn by their outline, creases and borders only".

On the screen the plan of an object is made once for each revision of the document (`surface_draw.rs`), with the faces already cut into triangles, and a frame only moves them to the screen. The cache keeps at most 8,192 plans. When it is full the plans of older revisions go first, then the quarter that was used least recently. What is outside the window is not added. A frame has a budget of 400,000 mesh vertices. An object that would pass it is drawn as its box, with the note "Drawn as box: the frame budget is used up". A ghost during a drag is the same drawing at an offset, and a ghost of a compare is not kept.

### 3D view, renders and the look

The 3D view, the renders and the modeler build the scene with the same function, so the look is the same: the front and the back colors, the materials, the render styles, the smooth shading across smooth edges and the edge lines. An instance draws the parts of its definition. A part with a class that is switched off is left out, and a part gives its own fill (or the fill of its class) to its faces. In a viewport the classes are those of the viewport.

The soft and the hidden edges are kept in `Scene::soft_edges`. They are not lines. A pass that finds creases in the mesh (the hidden-line passes and the GPU hidden-line mode) does not make a crease of them, so the sides of a softened cylinder are not drawn. A silhouette is still drawn, so the outline of the cylinder stays.

### Select, move, snap

- A click picks the footprint: inside a face that is seen from above, or within the pick distance of an edge that is shown. A model that is an L is not picked in the notch of the L. A model that the plan draws as an outline is picked in all its footprint. A window and a crossing use the box of the object.
- A hidden class, a grey class and a locked layer cannot be picked. This is the rule `kadakas_ui_core::hit::pickable`. The plan and the 3D view use it both. A click in the 3D view on a thing that cannot be picked selects nothing.
- A click in the 3D view picks the object under the pointer. A member of a group selects the group, as in the plan, unless you are inside the group.
- Move, Rotate, copy and delete are the commands of any object (`move_objects`, `rotate_objects`, `copy_objects`, `delete_objects`, `transform_objects`) and one undo step each. A model changes its `transform`. An instance changes its `transform`. A group moves as one. A copy of an instance shares the definition.
- The snap finds the vertices (endpoint), the middles of the edges that are shown (midpoint) and the centers of the faces that are seen from above (center). An edge snaps as an edge. Hidden and soft edges give nothing. Only what is near the pointer is made, and no more than 20,000 edges and 20,000 centers of one model. A model that the plan draws as an outline or a box offers the points of that drawing only, so the pointer is never offered a vertex that is not on the screen.

### Object Info

The palette shows, for a surface model, a group of them and an instance: the kind, the name of the model, the numbers of models (more than one), vertices, edges and faces, the solid state, the volume and the area, the size and the lowest and the highest corner, the hidden faces and edges, the soft edges, the Plan row when the plan simplifies the model, and the materials with the number of faces of each (a material on both sides of a face counts the face once). The numbers come from `kadakas_core::surface::info`. They are in the units of the person and in the drawing: the transform of a model scales the volume and the area. The volume of a group is the sum of the volumes of its solids, so solids that overlap count twice.

The solid state is one of `Closed solid` (every shell is a solid and no edge is loose), `Not closed` (with the number of open edges), `Faces cross each other`, `Closed, but the faces look inward`, `Closed, but it holds no volume`, `No faces` and `Not checked`. The check takes up to 3 s for a model at the limits (`docs/modeler-limits.md`, D1), so it has a budget. One answer checks at most 20,000 faces in all (a list of objects 60,000), and the models that do not fit are `Not checked`. The palette then shows a button, Check Solid. The verdict is kept for the content of the model (its vertices, edges and faces), not for the revision of the document. So an edit of something else does not check the model again, and a copy of the model costs nothing.

The fields are read only. A line says that the geometry is edited in the 3D modeler. The button "Open in the 3D Modeler" is shown when the program `kadakas-modeler` is next to the app. The modeler opens the saved file, so the status line asks for a save first when the drawing is new or has changes. The path is made whole first (`std::path::absolute`), because the modeler has a working folder of its own. If a modeler runs, the file goes to it as an open request, as in the other direction (`kadakas_update::launch::open_in_sibling`).

### Sheets, hidden-line output and sections

- A plan viewport draws the plan above, with the colors and the pens of the viewport. A class or a layer that the viewport hides hides the model, and the parts of an instance follow the classes of the viewport.
- A 3D viewport (top, elevation, isometric) draws the scene. In a hidden-line mode the soft edges are not creases. An object with triangles and more than 50,000 lines (`hlr::MAX_OBJECT_LINES`) is drawn by the silhouettes, the creases and the borders of its mesh, not by every edge. The line list of a pass is held to 150,000 lines (`hlr::MAX_LINES`) the same way. `HiddenLines::thinned_objects` and `SectionDrawing::thinned_objects` say how many objects it concerned, and the page says it.
- A section cuts the faces of a model that close a volume to a ring and fills it with the poche, with the outline of the cut. The test is cheap and is made for every model, whatever its size: every edge of the faces that are not hidden is used twice, once in each direction (`surface::info::is_watertight`). The faces of a model that fail it are cut to a line and are not filled, also when the ring closes (a box without a wall that is parallel to the cut). The scene marks the triangles of such a model (`Scene::open_tris`), so each part of an instance is cut by its own state: a closed box and an open box in one instance give a filled ring and a line. The edges beyond the cut are the edges of the model that are shown, with hidden parts removed. Soft edges are left out.
- PDF, SVG and PNG of a sheet carry all of it. DXF export of the model writes a surface model as one 3D mesh and an instance as one mesh for each part, through the transform of the instance.

### Agent API

`objects.list` and `objects.get` give every surface model, group of them and instance the field `surface`: the counts, `solid` (`closed` with `volume_mm3`, `open` with `open_edges`, `crossing`, `inward`, `flat`, `unchecked` or `empty`), `area_mm2`, `bounds`, the materials, `unpainted_faces` and, when the plan simplifies it, `plan`. The list leaves the geometry of a surface model out (`shape.elided`) and nothing else: an instance keeps its `symbol` and a group its shape. `objects.get` has the geometry. A list checks at most 60,000 faces for a closed solid in all, and a model that was checked before and has not changed costs nothing. `describe` has the text in `mesh.in_the_app`.

### Tests

| What | Where |
|---|---|
| The plan: faces, colors, order of overlapping faces, wall edges under a roof, hidden and soft, holes, mirror, the limits, picking, snapping (also of a big model), the numbers and states of Object Info, the budget and the cache of the solid check | `crates/kadakas-core/src/surface/tests_plan.rs` |
| SVG, snapping, visibility, the view of the classes, nested and turned instances, move, turn, copy, delete and undo of models, instances and groups, the 3D scene | `crates/kadakas-core/tests/surface_plan.rs` |
| Select tool, footprint, window, classes, snap | `crates/kadakas-ui-core/tests/surface_objects.rs` |
| The canvas: the colors at points of the mesh, soft edges, ghosts, the cache and its eviction, outline and its note, the budget of a frame, broken instance | `crates/kadakas-app/src/surface_draw.rs` |
| Object Info: rows, states, the Plan row, the panel, the check button, Open in the 3D Modeler (also a relative path), Estonian notes | `crates/kadakas-app/src/surface_ui.rs` |
| Picking in the 3D view: groups, instances, grey and locked | `crates/kadakas-app/src/canvas3d.rs` |
| Sheets: plan viewport, material, hidden class and the classes of a viewport (plan, 3D and section), instance, section area, poche, a hole through a box, a missing wall, the parts of an instance, soft edges in hidden line, the notes on the page, in the SVG and in the PDF | `crates/kadakas-print/tests/surface_sheets.rs` |
| The thinning of lines in a hidden-line pass | `crates/kadakas-render/src/hlr.rs` (tests) |
| DXF of instances, both modes of `visible_only` | `crates/kadakas-io/tests/dxf_surface.rs` |
| The API: the list, the symbol of an instance, the notes, the budget | `crates/kadakas-api/tests/mesh_app.rs`, `python/tests/test_mesh.py` |
| Timing of the drawing at 100,000 faces and of 5,000 instances | `crates/kadakas-print/tests/surface_perf.rs`, `time_of_the_plan_on_the_screen` and `time_of_5000_instances` in `surface_draw.rs` (all `--ignored`) |

### Not done

- A dashed or ghosted display of hidden geometry in the plan.
- A face is not cut where heights cross inside an overlap of two faces of the plan (see "The plan").
- A section does not look at a hole in a hidden face: hidden faces make a model open, as in the scene.
- A part of an instance on a grey class is drawn and snapped to as if the class were visible.
- A plan of a big model is the outline at every zoom. It does not show more when you zoom in.
- Editing geometry in the app. The modeler does it.

## Clean room

The layout, the tool names, the logic of the tools and the default keys follow public documentation of programs of this kind (`docs/research/sketchup-public-features.md`). No program of that kind was downloaded or run. No icon, image or text was copied. SketchUp is a trademark of Trimble Inc. The About window says that Kadakas Modeler is not affiliated with it.

## Not done yet

- A window of the web build. The program is for the desktop. The 3D view and the tray need a native window and a file dialog.
- The rest of MOD-09: solids in different groups or components (each is a model of its own, so the operation needs both solids in one model), solid tools in the context menu, and a tool that picks the two solids by clicks.
- The rest of MOD-08: Intersect Faces With Context, coplanar overlaps, rounded corners for Offset, a Follow Me profile that touches the path inside a side, and check boxes for the edge flags in Entity Info (they are read only).
- Not tried in a real window: the drawing, the marks and the hatch on a GPU window were checked in headless frames and in one GPU test of the back color, not by hand. The same holds for the Paint Bucket (the WGSL shader that draws one side of a triangle is checked by reading and by the CPU renderers, not on a GPU), the thumbnails and the editor of the Materials panel, and "Open in the Kadakas app" (the start of the program is tested with a script in a temporary folder, not with a real app).
- An editor for the keys of the modeler, and a review window for crash reports. A report stays in the queue until `kadakas-app` starts.
- Autosave and the agent server of the app.
