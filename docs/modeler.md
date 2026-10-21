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

Only Select and the navigation tools run today. The other tools are in the tool bar, disabled. Their tooltip says "coming in a later version". Their keys show the same note in the status bar. The rows MOD-03 to MOD-05 and MOD-08 turn them on one by one.

**Measurements box.** It only shows a value today. It shows the size of the selection (length, width and height in the units of the person). While the Zoom tool is active, it shows the field of view. MOD-04 lets you type into it.

**Entity Info.** It shows the type, the id, the layer, the class, the size and the lowest corner of the selected object. For several objects it shows the count and the size of the box around them. With no selection it shows the active layer and class. **Materials** and **Scenes** are placeholders. Paint comes with MOD-05 and scenes with MOD-10.

**Unsaved changes.** The session knows when the model has changes that nobody saved. File > New, File > Open, a file from the launcher, File > Quit and the close button then ask the same three questions as the app: Save and open, Don't save, or Cancel. Only one question is open at a time. A second request is dropped, and the status line says so. Today the modeler cannot edit, so a model has unsaved changes only after an undo or a later tool.

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

## Surface models (MOD-06)

The modeler shows every object that the scene builder of `kadakas-core` turns into triangles and lines. A surface model (`Shape::SurfaceModel`, MOD-06) was not in the core when this page was written. When it lands, it appears in the 3D view without a change of the modeler, because the scene builder makes its mesh. Two small places need a line each:

- `kadakas_ui_core::modeler::entity_info` (look for `HOOK(MOD-06)`): add the number of vertices, edges and faces to Entity Info.
- `kadakas-modeler/src/tray.rs`: show those numbers.

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

**Edit in place.** `edit_component` turns the parts of the definition into objects in an edit group at the instance (placed by the instance transform) and hides the instance. Every command then works on those objects: draw, make a group, make a component, explode, nest. `close_component` writes the objects back as the parts and removes the edit group; all instances show the new definition at once. While the edit is open, the other instances still show the old definition (a live update is a later step of the UI). A definition is open in one place at a time, and an outer edit cannot close while an inner one is open. The edit group keeps the version of the definition (`definition_version`, a hash of its parts). If another user changed the definition meanwhile, `close_component` fails with a message that starts with `conflict:` and changes nothing; the person answers with `resolve`: `keep_as_new` keeps the edit as a new component `Name (mine)` that the instance uses (the other version stays), `discard` throws the edit away. Nothing is overwritten silently.

**Raw model and edit context.** The raw geometry of a level (the root of the drawing, a group, an edit group) lives in its surface model objects. The *raw model* is the first of them in z-order. Drawing in a context goes there, and an exploded group merges into it. `EditContext` (headless, in the core) keeps the path of ids that a person has entered, enters a group (no change of the document) or a component (`edit_component`), leaves it (`close_component`), and answers what the UI needs: `container`, `contains`, `pick`, `dimmed` (the flag `dim_rest` says whether the rest is drawn dimmed), `raw_model`, `to_local` and `to_world`. `apply_world` takes an edit with points in the drawing, converts them into the local coordinates of the raw model through its transform, and, if the level has no model yet, makes one in the same undo step.

**Limits.** 32 groups deep, 16 components deep, 20,000 surface models in the expansion of one definition, 10,000 parts per definition. A command or a walk over a limit fails with a message (`ComponentError`) and changes nothing; a component that would contain itself is refused. `add_resource` and `modify_resource` of a symbol with parts check the expansion of the definition and of every definition that uses it (a nested definition that grows can fail an outer one), and the 2D `group`, `make_group`, `edit_component` and `explode` keep 32 groups. A file with a definition over a limit loads, and the load notes say which; its instances are drawn as a marked cross. The measurement is `perf_limits` in `crates/kadakas-core/tests/components_files.rs` (debug build: making 32 nested groups 5 ms, moving them 2 ms, walking a chain of 16 definitions 2 microseconds, walking 20,000 models 4 ms).

**Inference.** `kadakas_ui_core::infer3d::scene_of_instances(doc, instances, first_model)` makes one `Scene` of any number of instances. A definition is turned into a `DefinitionScene` once (in the coordinates of the definition, nested components placed) and kept in a `DefinitionCache`; each instance adds a placed copy of the points, edges and faces. The surface models of the definition are never copied. The model number of a tag is `first_model` plus the ordinal of the model in the definition; `InstanceScenes::instance_of(tag)` turns it back into the instance. The cache keeps the content version of a definition with its scene (`component::version`: revisions of the surface models, placements, nested definitions), so an undo, a change by a peer and a change of a nested component are seen without any call; `forget` only frees memory.

**Collaboration and files.** Groups and instances are objects and definitions are resources, so the commands need no new rule. The surface models are changed with `SurfaceModel::apply`, so `rev` and the log of MOD-06 stay true. In the file a definition has `parts` in its symbol, an instance is `{"kind": "component", "symbol": ..., "transform": [...]}`.

**Broken instances.** An instance whose definition is missing, is not a component, contains itself or is over a limit is not dropped and not hidden: the plan, the SVG, the print and the 3D scene draw a cross in the warning colour at its origin, `component::findings` and `document.summary.broken_components` give the reason, and the load notes list it. A component instance may only name a definition with 3D parts; a symbol instance may not name a definition that has only parts (a mixed symbol with 2D items and parts serves both).

**Modeler.** The Edit menu has Make Group, Make Component, Explode, Make Unique, Edit Group or Component and Close Group or Component. They work on the selected objects (face and edge selection comes with MOD-05) and show the message of the core when a command is refused. A new component is named `Component 1`, `Component 2`, ... (a name box comes later).

**Not done.** Glue To, Cut Opening, Reload and Save As, Always Face Camera, Live and Dynamic Components, Reset Scale and Skew, Change Axes, lock, a live update of the other instances while an edit is open, and the plan drawing of components (MOD-13: the plan shows the box around an instance, the same on the screen, in the SVG and in the print).

## Clean room

The layout, the tool names, the logic of the tools and the default keys follow public documentation of programs of this kind (`docs/research/sketchup-public-features.md`). No program of that kind was downloaded or run. No icon, image or text was copied. SketchUp is a trademark of Trimble Inc. The About window says that Kadakas Modeler is not affiliated with it.

## Not done yet

- A window of the web build. The program is for the desktop. The 3D view and the tray need a native window and a file dialog.
- Editing: the tools that draw and change geometry (MOD-03 to MOD-05 and MOD-08).
- The inference engine (MOD-03) and typing into the Measurements box (MOD-04).
- An editor for the keys of the modeler, and a review window for crash reports. A report stays in the queue until `kadakas-app` starts.
- Autosave and the agent server of the app.
