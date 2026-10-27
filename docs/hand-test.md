# Hand test before the first beta

This is a checklist for one person at a real screen. The code ran only in unit tests, headless render tests and CI smoke tests. No person has used it in a real GPU window yet. This list finds what those tests miss.

Print it or copy it. Tick a box when the step passes. When a step fails, write the note in the last column and go on. Do not stop the session for one failure.

## How to use this list

- Do the sessions in order. The first sessions hold the most valuable and most fragile steps.
- Each session takes 30 to 45 minutes.
- Every step has four parts: what to do, what happens, a box, and what to write down if it fails.
- A step marked **NOT BUILT** needs a feature that is not in the program yet. Skip it. The status comes from `docs/feature-spec.md`.
- A step marked **(Win/Linux)** needs that system. Skip it when you have no such machine.
- Keys: `Cmd` on a Mac, `Ctrl` on Windows and Linux.
- Report every failure with the template at the end ("How to report").

## What the CI smoke test already proves

The job `smoke` of `release.yml` (`docs/release.md`, "Smoke test of the installers") runs on clean machines of each system. Do not repeat these checks by hand.

| Already proven | Not proven (this list covers it) |
|---|---|
| Every package installs and uninstalls (MSI, NSIS, dmg for arm64 and Intel, deb, AppImage) | Gatekeeper and SmartScreen warnings |
| The launcher, the app, the modeler and the `kadakas` tool exist in the package | Real GPU rendering (CI uses software renderers) |
| The license texts are in the package | Double click on a `.kad` file, the Dock, high DPI, trackpads |
| The menu entry starts the launcher (`Info.plist` has name, icon and the `.kad` type) | Anything a person draws, clicks or types |
| First start: the welcome dialog opens, answers "Use defaults", and writes `welcome.json` and the settings files | Looks of the dialog, focus, Tab order, screen reader |
| Second start: no dialog, the 3D modeler starts and stays up for 12 seconds | Whether the modeler window is usable |
| Third start: the app answers `auth` and `describe` over TCP with the right version | Agent edits and the review in the window |
| A silent fresh install returns no restart code | A silent upgrade over a running app (PLAT-29, not tested) |

## Prepare the Mac

### Disk space

| What | Space |
|---|---|
| Release build (`target/release`) | about 2 GB |
| Debug build, if you also run `cargo test` (`target/debug`) | about 2 GB |
| Packages in `dist/` (app bundle and dmg) | about 1 GB |
| Test files, recovery copies, logs | under 1 GB |
| Free space to keep before you start | 10 GB |

A first build from nothing takes longer and uses more space than the numbers above. The cargo cache in `~/.cargo` is extra. Check with `df -h /`.

### Option A: run from the build (fastest)

Run these in the repo folder. Do not run them inside `.claude/worktrees`. The paths in the packager config expect the main checkout.

```sh
cargo build --release -p kadakas-app -p kadakas-modeler -p kadakas-cli -p kadakas-launcher
cargo run --release -p kadakas-app                       # the 2D landscape app
cargo run --release -p kadakas-modeler -- model.kad      # the 3D modeler (the file is optional)
cargo run --release -p kadakas-app -- --agent-port 0     # the app with the agent server
```

Set `KADAKAS_SKIP_WELCOME=1` to skip the welcome dialog. Do not set it in Session 1.

### Option B: run as an installed app (closest to a user)

```sh
cargo install cargo-packager --locked
cargo build --release -p kadakas-app -p kadakas-modeler -p kadakas-cli -p kadakas-launcher
scripts/macos-app.sh target/release dist-app
cargo packager --release --config packager-macos.toml --formats app,dmg
```

- The `.dmg` step runs a Finder script. Run it from a logged-in desktop session, not over SSH.
- The files go to `dist/`. Drag `Kadakas.app` to `/Applications` from the dmg.
- The build is not signed. Gatekeeper warns at the first start. That is one of the steps below.

### Option C: install into the launcher store

`scripts/install-local-build.sh` builds the release and puts a whole app bundle into the version store of the launcher with the label `local`. Use `kadakas-launcher manage` to start it.

### Files to open

| File | What it is |
|---|---|
| `tests/behavior/fixtures/parking_lot.dxf` | Small DXF of a parking lot. Good first DXF import (18 KB). |
| `tests/behavior/fixtures/parking_lot_loose.dxf` | The same lot with loose lines. |
| `tests/behavior/fixtures/survey_rev1.dxf` and `survey_rev2.dxf` | Two revisions of a survey (100 KB each). For Update from DXF. |
| `tests/fixtures/ifc/all_kinds.ifc` | IFC with many kinds of object (30 KB). |
| `tests/fixtures/ifc/wall_with_openings.ifc`, `terrain_triangulated.ifc`, `vegetation_plant.ifc` | Wall, terrain and plant in IFC. |
| `tests/fixtures/ifc/truncated.ifc`, `unsupported_bbox.ifc` | Broken and unsupported IFC. Expect a clear message. |
| `tests/data/step/box_metre.step`, `tessellated.step` | Small STEP files. |
| `crates/kadakas-api/tests/data/container_golden.kad` and `versions_golden.kad` | Real `.kad` containers from older code. Test that old files open. |
| `docs/scripts/radial-fan.js`, `random-ashlar.js` | Pattern generator scripts for the Scripts and JS steps. |
| `python/examples/plugins/stepping_stones.py` | Python plug-in example. |

There is no sample drawing in the repo. In Session 1 you make one and keep it as `~/Desktop/hand-test/garden.kad`. Make the folder first.

### Where things are stored (macOS)

| What | Place |
|---|---|
| Settings (`prefs.json`, `modeler.json`) | `~/Library/Application Support/kadakas/` |
| Recovery copies (autosave) | `~/Library/Application Support/kadakas/recovery` (`KADAKAS_RECOVERY_DIR` changes it) |
| Launcher folder: `launcher.json`, `welcome.json`, `runs/`, `crash-reports/`, `versions/` | `~/Library/Application Support/kadakas/launcher` (`KADAKAS_LAUNCHER_ROOT` changes it) |
| Agent runtime file `agent.json` | `~/Library/Application Support/kadakas/agent.json` (or `$XDG_RUNTIME_DIR/kadakas/`) |
| Failed-save log `save-failed.log` | the state folder, same place as the settings on a Mac |
| Windows | `%APPDATA%\kadakas`, `%LOCALAPPDATA%\kadakas` |
| Linux | `~/.config/kadakas`, `~/.local/state/kadakas`, `~/.local/share/kadakas` |

Check the launcher folder with `ls ~/Library/Application\ Support/kadakas`. If your folder differs, write the real path in the first bug report.

---

## Session 1: First start (35 minutes)

Goal: the program starts, looks right, saves and reopens a file. If this fails, nothing else matters.

| # | Do | Expect | OK | If it fails: write down |
|---|---|---|---|---|
| 1.1 | Move old settings away: `mv ~/Library/Application\ Support/kadakas ~/Desktop/kadakas-old` (skip on a clean Mac). Start the build (Option A or B). | The welcome dialog opens first, before any other window. It comes in front of Finder. | [ ] | Screenshot. Did a second Dock icon stay? Terminal output. |
| 1.2 | Look at the welcome dialog. Read all the text. | Three cards: how to work (3D Modeler or Landscape CAD), language, units with number format. Defaults follow your system. Text fits. No clipped words. | [ ] | Screenshot. System language and region. |
| 1.3 | Press Tab through the dialog. | Order: cards, language, units, number format, More options, options inside, Use defaults, Start. A focus ring shows on each. | [ ] | Where the focus jumped or vanished. |
| 1.4 | Open "More options". Change theme to Dark. | The dialog turns dark at once. | [ ] | Screenshot. |
| 1.5 | Change the language to Estonian, then back to English. | Text and window title change at once. The number format follows the language until you choose one. | [ ] | Which texts stayed English. |
| 1.6 | Choose Landscape CAD. Press Start. | The 2D app window opens. The window has a menu bar, tool bar and a drawing area. | [ ] | Screenshot. Time to open. Terminal output. |
| 1.7 | Look at the first window. Drag its edge to resize. Maximize it. Quit and start again with `KADAKAS_SKIP_WELCOME=1` set. | Drawing area redraws without flicker or black bars. After the restart the dialog does not come again. | [ ] | Screenshot of any artifact. GPU name (Apple menu > About This Mac). |
| 1.8 | Press `Cmd+Q` (or File > Quit) at once. Start again. | The app closes. No crash dialog on the next start. | [ ] | Whether a crash or recovery dialog showed. |
| 1.9 | Menu Help > About Kadakas. | Shows name, version, license, and the third-party licenses. Write the version here: ________ | [ ] | Screenshot. |
| 1.10 | Menu File > New. Draw a rectangle with the Rectangle tool (key `4`) and a line (key `2`). | Shapes appear while you move the pointer and stay after the click. The status line shows hints. | [ ] | Which tool, what you saw. |
| 1.11 | Press `Cmd+Z`, then `Cmd+Y` (or `Cmd+Shift+Z`). | The last shape goes and comes back. | [ ] | Which step failed. |
| 1.12 | Menu File > Save As. Name it `garden.kad` in `~/Desktop/hand-test/`. | The title shows `garden.kad`. The file exists in Finder. | [ ] | The dialog text. |
| 1.13 | Quit. Start the app again. File > Open `garden.kad`. | The same two shapes show. The view is the same. | [ ] | What changed. |
| 1.14 | Open `crates/kadakas-api/tests/data/container_golden.kad`. | The file opens, or a clear message says why not. | [ ] | The exact message. |
| 1.15 | Quit the app. Start the 3D modeler: `cargo run --release -p kadakas-modeler -- ~/Desktop/hand-test/garden.kad`. | The modeler window opens with a 3D view, menu (File, Edit, Camera, Tools, View, Help), tool bar and tray. The two shapes show in 3D. | [ ] | Screenshot. Terminal output. |
| 1.16 | In the modeler, hold the middle mouse button and drag. Scroll the wheel. Press `Shift+Z`. | The view orbits, zooms to the pointer and fits all. | [ ] | Mouse model. What was wrong. |
| 1.17 | Run `scripts/install-local-build.sh`, then `target/release/kadakas-launcher manage` (Option C). | A window lists the `local` version with its commit. It starts from there. | [ ] | Window text. |

## Session 2: 2D landscape workflow (45 minutes)

Goal: the daily work of a landscape architect. Start the app with `cargo run --release -p kadakas-app`.

| # | Do | Expect | OK | If it fails: write down |
|---|---|---|---|---|
| 2.1 | File > New. Open the Layers palette (look in the Window menu). Add a layer `Planting`. Make it active. | The layer shows in the list and is active. | [ ] | Where the layers palette is. |
| 2.2 | Draw a closed polyline (key `5`). Press Enter to finish. | A closed shape. Object Info shows its kind and area. | [ ] | Screenshot. |
| 2.3 | Select the shape. Open Object Info (look in the Window menu). Change a value (for example the layer or fill). | The drawing updates at once. Undo returns the value. | [ ] | Field name. |
| 2.4 | Lock the layer, then try to move a shape on it. Unlock. | A locked layer refuses edits with a message. | [ ] | The message. |
| 2.5 | Hide the layer, then show it. Set it to grey. | Hidden objects vanish. Grey shows faint without color. | [ ] | Screenshot. |
| 2.6 | Open the Resource Manager. Create a plant style (name, spread, height, colors). | The style shows in the list. | [ ] | Fields that failed. |
| 2.7 | Choose the plant tool. Place a plant in a click, then fill your closed shape with plants (tool mode "fill an area"). | Plant symbols draw with the style color. Spacing follows the spread. | [ ] | Screenshot. Number of plants. |
| 2.8 | Open the plant schedule (Window or Data palette). | Counts per style match what you placed. | [ ] | Counts you expected and saw. |
| 2.9 | Place a plant tag on a plant. | The tag shows the ID code and count with a leader line. | [ ] | Screenshot. |
| 2.10 | Press `Alt+Shift+I` for the Irrigation tool. Add a point of connection, a valve, a mainline and three spray heads (mode Outlet, Line, POC, Valve). | Symbols and lines draw. `U` cycles the modes. | [ ] | Mode that failed. |
| 2.11 | In the irrigation palette, press Calculate. Open the zone schedule. | Heads join the nearest valve. Flow and pressure show numbers, or a warning that names the problem. | [ ] | Message text. |
| 2.12 | Add a sheet layer (look in the menus and palettes for Sheet). Add a viewport of your design layers at a scale. Add a title block and a north arrow. | The sheet shows a paper, the viewport and the title block. | [ ] | Screenshot. |
| 2.13 | File > Export PDF... (or `Cmd+P`). Save to the hand-test folder. Open it in Preview. | A PDF with vector lines, line weights and text. Lines stay sharp when you zoom to 800 percent. | [ ] | The PDF, if small. |
| 2.14 | File > Export SVG... Open the SVG in Safari. | The drawing shows. | [ ] | Screenshot. |
| 2.15 | Nudge a shape with `Shift+Arrow`. Press `Cmd+Z` five times, then `Cmd+Y` five times. | Each step moves one change. State after redo equals state before undo. | [ ] | Step where it differed. |
| 2.16 | Press `Cmd+S`. Quit. Start. File > Open the file. | Plants, irrigation, sheet and layers return. | [ ] | What was missing. |
| 2.17 | File > Import... Choose `parking_lot.dxf`. | A dialog with options. After Import, lines and areas show on layers. A report lists dropped items. | [ ] | Screenshot. The report text. |
| 2.18 | Import `survey_rev1.dxf`. Then use File > Update from DXF/DWG... with `survey_rev2.dxf`. | A window shows added, changed and removed objects. You choose what to apply. | [ ] | Screenshot. |
| 2.19 | Import `tests/fixtures/ifc/all_kinds.ifc` (File > Import...). | Objects appear. A report lists what is approximated or skipped. | [ ] | Report text. |
| 2.20 | Import `tests/fixtures/ifc/terrain_triangulated.ifc`. Switch to the 3D view. | A terrain surface shows in 3D. | [ ] | Screenshot. |
| 2.21 | File > Scripts... Choose the folder `python/examples/plugins`. Run `stepping_stones.py`. (Needs Python 3 and the `kadakas` package on the path: `pip install -e python`.) | The Console shows the output. Objects appear. Undo removes them. | [ ] | Console text. |

## Session 3: The 3D modeler (45 minutes)

Goal: the modeling tools on a real GPU. `docs/modeler.md` lists what no person tried yet. These are the drawing, the marks, the hatch, the Paint Bucket shader, the material thumbnails and "Open in the Kadakas app". Start with `cargo run --release -p kadakas-modeler`.

Status: MOD-01 to MOD-04, MOD-06, MOD-07, MOD-09 to MOD-13 are done. MOD-05, MOD-08 and MOD-15 are in progress or partial. MOD-14, MOD-16, MOD-17 and MOD-18 are **NOT BUILT**. Walk, Look Around, an editor for keys and autosave are **NOT BUILT** in the modeler.

| # | Do | Expect | OK | If it fails: write down |
|---|---|---|---|---|
| 3.1 | Press `R`. Click two corners on the ground. | A rectangle face. The front is a light color. Measurements box (bottom right) shows size while you move. | [ ] | Screenshot. |
| 3.2 | Orbit under the ground (middle drag). | The back of the face is bluish. No flicker or z-fighting. | [ ] | Screenshot of flicker. |
| 3.3 | Press `P`. Click the face, move up, type `2000` and Enter. | A box rises 2 m. Walls and top have faces. Measurements box takes the typed value. | [ ] | Value you typed, what you saw. |
| 3.4 | Press `C`, click a center, type `500` Enter. Then `P` on it and pull 1 m. | A circle (24 sides). It pulls to a cylinder. Entity Info shows true radius (MOD-15). | [ ] | Screenshot. |
| 3.5 | Press `A`, draw an arc: chord, then bulge. Type `12s` before the bulge. | An arc of 12 segments, one curve. | [ ] | Screenshot. |
| 3.6 | Press `L`, draw a closed loop on a box top. Press `E` and click an edge. | The loop makes a face. The eraser removes the edge and merges faces. | [ ] | Screenshot. |
| 3.7 | Draw a path with Line. Draw a profile. Choose Follow Me (Tools menu). | The profile sweeps along the path. | [ ] | Screenshot. |
| 3.8 | Press `F` (Offset). Click a face and move in. | An inner face at the distance. | [ ] | Value typed. |
| 3.9 | Press `M`. Click a box. Move with the arrow key locking an axis. Hold Ctrl (Option on a Mac) to copy. | The box moves along the red axis. A copy remains. | [ ] | Axis colors seen. |
| 3.10 | Press `Q` (Rotate) and `S` (Scale). Use each on a box. | A protractor shows. Scale handles show. | [ ] | Screenshot. |
| 3.11 | Press `T`. Click two points. Type a length. | A measure shows. A guide line stays. | [ ] | Screenshot. |
| 3.12 | Press `B`. Pick a material in the Materials panel. Click a face. Orbit around. | The face takes the color on the clicked side only. This tests the WGSL shader on a GPU. | [ ] | Screenshot of both sides. |
| 3.13 | In Materials, make a new material. Check the thumbnail and editor. | Thumbnails draw. The editor changes color. | [ ] | Screenshot. |
| 3.14 | Select a box. Make a group (look in the Edit menu or the right-click menu). Enter the group by double click. Make a component. | The group is one object. A component updates all instances when you edit one. | [ ] | Screenshot. |
| 3.15 | Make two overlapping closed boxes in one group. Use the solid tools (union, subtract, intersect). | The result is a closed solid. An open model gives a report of open edges, not a crash. | [ ] | Message text. |
| 3.16 | In the Scenes panel press Add. Move the camera. Add a second scene. Click each scene. | The camera moves in about half a second. A star shows after a changed scene name. | [ ] | Screenshot. |
| 3.17 | In Styles, change the render style. Save a scene. | The view changes. The scene remembers the style. | [ ] | Style names. |
| 3.18 | **Animation**: View > Animate Scene Changes. Switch it off and click scenes. | Scenes jump at once with it off. | [ ] | Setting state. |
| 3.19 | File > Import. Import an OBJ or STL file (make a cube in the modeler first and export it with File > Export...). | Import shows the "Soften edges" question and the units. The model appears with colors. | [ ] | Dialog text. |
| 3.20 | File > Export... in each of glTF, STL, IFC and DXF. Open the glTF in any viewer. | Files write without errors. | [ ] | Format that failed. |
| 3.21 | Import `tests/data/step/box_metre.step`. | A box in meters. | [ ] | Size seen. |
| 3.22 | `Cmd+Z` 10 times. `Cmd+Y` 10 times. | Each step undoes one edit. | [ ] | Step. |
| 3.23 | Save with `Cmd+S`. Open the file in the 2D app. | Surface models show in the plan and in 3D. The same scenes show in the Navigation palette. | [ ] | Screenshot. |
| 3.24 | In the modeler use File > Open in the Kadakas app. | The 2D app opens the file, or the running app takes it. | [ ] | What happened. |
| 3.25 | Select an edge, soften it in the Soften Edges panel. | Smooth shading across the edge. | [ ] | Screenshot. |
| 3.26 | Time check: draw 50 circles by copy and pull. Orbit. | The view stays smooth. | [ ] | Frame rate feel, GPU. |

## Session 4: Agent review (40 minutes)

Goal: the main use case. An agent edits, the person reviews in the window. The rows are AGR-02, AGR-03, AGR-05 and AGR-06. All say "not tried in a real window yet". AGR-04 (the person points to the agent) is **NOT BUILT**. The 3D surface tint of AGR-05 is **NOT BUILT**. The activity log is not saved with the file.

### Set up

1. In terminal 1 run `cargo run --release -p kadakas-app -- --agent-port 0`. Open `~/Desktop/hand-test/garden.kad`.
2. In terminal 2 check the runtime file: `cat ~/Library/Application\ Support/kadakas/agent.json`. It holds a port and a token.
3. Connect with Python: `pip install -e python`, then run `python3`:

   ```python
   import kadakas
   doc = kadakas.connect()
   print(doc.view_get())
   ```
4. For MCP: `claude mcp add kadakas -- /path/to/target/release/kadakas mcp --connect`. Then ask Claude Code to call `describe`.

| # | Do | Expect | OK | If it fails: write down |
|---|---|---|---|---|
| 4.1 | Run the Python snippet. | `view_get()` returns camera, zoom, visible box, selection, active layer. Connection works without a prompt. | [ ] | Error text. Contents of `agent.json` (hide the token). |
| 4.2 | Select a shape in the window. Run `doc.view_get()` again. | The selection shows its id. | [ ] | Output. |
| 4.3 | Run `doc.view_zoom(ids=[<id>], margin=0.15)`. | The view moves in about 250 ms and fits the object. | [ ] | Did it jump or fail? |
| 4.4 | Start a drag in the window. Run `doc.view_zoom(...)` while you hold the mouse. | The call fails with `person_navigating`. Your drag is not disturbed. | [ ] | Output. |
| 4.5 | Run `doc.view_camera([20000,-20000,12000],[5000,5000,0], projection="perspective")` in the 3D view. | The camera moves. Undo history does not change. | [ ] | Screenshot. |
| 4.6 | Press the Back button or `Cmd+Alt+B`. | The view goes back one agent change. | [ ] | Result. |
| 4.7 | Run `doc.view_highlight(ids=[<id>], text="Check this", color="red", until="next_change")`. | An outline glow with the text, in plan and in 3D. A cursor named Agent shows. Agent is in the People palette. A pointer panel lists it. | [ ] | Screenshot of plan and 3D. |
| 4.8 | Use all five colors once. | Each color has its own mark and line style. They differ without color. | [ ] | Colors that look alike. |
| 4.9 | Click the pointer in the panel. Dismiss it. Run `doc.view_highlight_clear()`. | Click zooms. Dismiss and clear remove markers. | [ ] | Result. |
| 4.10 | Let the agent edit: `doc.add.line([0,0],[5000,0])`, add a rectangle, move one object, delete one object. | Objects appear in the window at once. | [ ] | Which call. |
| 4.11 | Look at the drawing. | Added: green, solid, `+`. Changed: orange, dotted, `~`. Removed: red, dashed, `-`, as a ghost. A legend shows the counts. | [ ] | Screenshot. |
| 4.12 | Switch to 3D (or the modeler). | A box with the same pattern around each change. Counts at the edge. | [ ] | Screenshot. |
| 4.13 | Menu File > Agent Activity... (`Cmd+Alt+A`). | One step per call. Each shows time, method, summary and counts. A step "Before agent session" exists as a version. | [ ] | Screenshot. |
| 4.14 | Click a step, then click an object in it. | Only that step shows. The object is selected and zoomed. The properties that differ show (`shape.a: [0,0] -> [5,0]`). | [ ] | Screenshot. |
| 4.15 | Press `Cmd+Alt+]` and `Cmd+Alt+[`. | Next and previous change zoom and select. The status line names the step. | [ ] | Result. |
| 4.16 | Edit an agent-added object by hand. | The step shows "edited since". | [ ] | Result. |
| 4.17 | Press Revert on one step. | The object goes back. Revert is one undo step of yours. | [ ] | Message text. |
| 4.18 | Press Undo to here on an older step. Then redo. | All later steps go and return. A question shows if your own steps are among them. | [ ] | Result. |
| 4.19 | After Undo to here with your own steps, call `doc.add.line(...)` from Python. | The call fails with `redo_pending`. A notice in the drawing has both buttons. | [ ] | Output. |
| 4.20 | Press `Cmd+Alt+S`, then `Cmd+Alt+H`. | All marked seen. The highlight switches off and on. | [ ] | Result. |
| 4.21 | With unsaved changes in the window, call `doc.execute({"op":"file.open", ...})` or `file.open` for another file. | The call is refused with `unsaved_changes`. The window asks "An agent wants to open X" with three buttons. | [ ] | Dialog text. |
| 4.22 | Run `script.eval` with `docs/scripts/radial-fan.js` (see `docs/scripting-javascript.md`). | One script run is one step. Undo takes all back in one step. | [ ] | Script output. |
| 4.23 | Run a long loop of 200 edits. | The window stays smooth. The review redraws at most every 300 ms. | [ ] | Feel, GPU. |
| 4.24 | Quit the app while the agent is connected. | The agent gets a closed connection. `agent.json` is deleted. | [ ] | Whether the file stayed. |
| 4.25 | Run `kadakas mcp --connect` through Claude Code. Ask it to add a plant and highlight it. | The same results as Python. | [ ] | Tool call and error. |

## Session 5: Platform extras (40 minutes)

| # | Do | Expect | OK | If it fails: write down |
|---|---|---|---|---|
| 5.1 | Install the dmg (Option B). Open `Kadakas.app` from Applications by double click. | Gatekeeper warns (unsigned). Right click, Open works. No second Dock icon after the welcome dialog. | [ ] | Warning text. macOS version. |
| 5.2 | In Finder double click `garden.kad`. | The app opens with the file. If the welcome dialog shows first, the file opens after Start. | [ ] | What opened. |
| 5.3 | With the app running, double click another `.kad` in Finder. | The running app opens it. No second app starts. It asks first if the drawing has unsaved changes. | [ ] | Result. |
| 5.4 | Click the Dock icon while a dialog is open. | The dialog comes forward. | [ ] | Result. |
| 5.5 | Right click the Dock icon. Choose Options > Keep in Dock. Quit, then start from the Dock. | The Kadakas icon starts the app. | [ ] | Which icon started. |
| 5.6 | Help > Show welcome. Change units to Imperial and the number format. Press Start. | The running program changes at once. Lengths show feet and inches. Number fields read and write the new format. | [ ] | Fields that did not change. |
| 5.7 | File > Preferences... Open each page. Look at Updates, Privacy and Autosave. | Pages open. Values save. | [ ] | Page. |
| 5.8 | Preferences > language Estonian. | The menus, palettes and dialogs change at once. Long words fit. | [ ] | Screenshots of clipped text. |
| 5.9 | In Estonian, type `1,5` and `1.5` into a length field. | Both read as 1.5. | [ ] | Result. |
| 5.10 | Set number format to `1 234,56`. Open the plant schedule and Object Info. | The UI shows the format. Dimension text and printed cells keep the drawing format (PLAT-27, known). | [ ] | Place that differs more. |
| 5.11 | Modeler: View > Language Estonian, View > Theme Dark. | Applies at once. Kept after a restart. | [ ] | Result. |
| 5.12 | Move the window to an external monitor with a different scale (or change the scale in System Settings > Displays). | Text and lines stay sharp. No blur, no wrong size. | [ ] | Scale values. |
| 5.13 | Trackpad. Modeler View > Mouse or trackpad > Trackpad. Two-finger scroll, `Shift` scroll, pinch. | Scroll pans. Shift tilts and turns. Pinch zooms. Momentum is smooth. | [ ] | Pan direction, speed. |
| 5.14 | Switch to Mouse. Use the wheel. | The wheel zooms as before. | [ ] | Result. |
| 5.15 | Keyboard only: open menus with the keyboard, Tab through a dialog, `Esc` closes. | You reach all buttons. | [ ] | Where focus is lost. |
| 5.16 | Turn on VoiceOver (`Cmd+F5`). Open the welcome dialog (Help > Show welcome). | Cards read as radio buttons with description and state. Drop-downs read with name and value. | [ ] | What was read. |
| 5.17 | Dark mode: change macOS to Dark. Theme System in both programs. | Programs follow. | [ ] | Screenshot. |
| 5.18 | Open the web build: `scripts/build-web.sh --serve`, open `http://127.0.0.1:8080` in Safari and Chrome. | The app shows. See `docs/web.md` for what does not work. It never ran in a browser. | [ ] | Browser, error in the console. |
| 5.19 | **(Win)** Install the MSI or NSIS. Double click two `.kad` files at the first start. | One welcome dialog. SmartScreen warns once. The dialog comes in front of Explorer. | [ ] | Screenshot. |
| 5.20 | **(Win/Linux)** Run Sessions 1 to 3 in short form: start, draw, save, reopen, modeler, orbit. | Same results as the Mac. | [ ] | OS, GPU, driver. |
| 5.21 | **(Linux)** Start on X11, then Wayland. Start with no display (SSH). | Both draw. No dialog without a display and a clear message. | [ ] | Session type. |
| 5.22 | **(Win/Linux)** Scale 100, 125 and 150 percent. 1366x768 at 125 percent with Estonian. | Buttons stay in reach. Content scrolls. | [ ] | Screenshot. |
| 5.23 | **(Win/Linux)** A Windows precision touchpad: pan, tilt, pinch. | Same as the Mac. | [ ] | Result. |
| 5.24 | Upgrade over a running app: install N, start it, install N+1 (PLAT-29). | The running app gets a close request and a chance to save. | [ ] | Exit code. **Needs two builds.** |

## Session 6: Failure paths (40 minutes)

Goal: the program fails in a safe way. Back up your own work first.

| # | Do | Expect | OK | If it fails: write down |
|---|---|---|---|---|
| 6.1 | Start the app via the launcher (Option B or C). Draw 3 shapes. Wait for the autosave (5 minutes or 50 operations). Check `ls ~/Library/Application\ Support/kadakas/recovery`. | Recovery files exist and your own file stays unchanged. | [ ] | Folder content. |
| 6.2 | Find the process id: `pgrep -f kadakas-app`. Run `kill -SEGV <pid>`. | The window disappears at once. The process ends in 5 seconds at most. | [ ] | Time it took. |
| 6.3 | Start the launcher again. | A crash window shows the summary, a note field, a contact field and the exact text that goes out. Buttons Send, Do not send, Always send. | [ ] | Screenshot. |
| 6.4 | Read the text. Look for your home folder name, file names and e-mail addresses. | They are scrubbed. Paths show `~` or placeholders. | [ ] | Any private text in the report. |
| 6.5 | Press "Do not send". Check `ls ~/Library/Application\ Support/kadakas/launcher/crash-reports`. | Nothing is sent. Report files stay or go as `docs/crash-reports.md` says. | [ ] | File list. |
| 6.6 | After the crash the app offers the recovery copy. Open it. | The shapes return. Open, discard and show folder all work. | [ ] | Result. |
| 6.7 | Run `kill -9 <pid>` (no signal handler). Start again. | No report. The recovery copy is offered. | [ ] | Result. |
| 6.8 | Start the app from a terminal and press `Ctrl+C`. | It ends. Next start has no crash window. | [ ] | Result. |
| 6.9 | Same crash test for the modeler: `kill -SEGV` on `kadakas-modeler`. | The report names `kadakas-modeler`. The next start of `kadakas-app` shows it. There is no autosave in the modeler, so no recovery (known). | [ ] | Result. |
| 6.10 | Set Preferences > Privacy > Crash reports to Never. Crash again. | No question. Reports are deleted. | [ ] | Folder content. |
| 6.11 | Corrupt a file: `cp garden.kad bad.kad`, then `head -c 200 garden.kad > bad.kad`. Open `bad.kad`. | A clear message. No crash. The open drawing stays. | [ ] | Message text. |
| 6.12 | `echo "hello" > fake.kad`. Open it. | "Not a Kadakas file" or a clear message. | [ ] | Message text. |
| 6.13 | Open `tests/fixtures/ifc/truncated.ifc` through File > Import. | A clear message. The program stays up. | [ ] | Message text. |
| 6.14 | Import `tests/fixtures/ifc/unsupported_bbox.ifc`. | A report lists the skipped items. | [ ] | Report text. |
| 6.15 | Make the file read-only (`chmod 444 garden.kad`). Edit and Save. | A clear message. Your work stays open. Save As works. | [ ] | Message text. |
| 6.16 | Save to a full or missing disk (`hdiutil` small image or eject a USB stick while saving). | A clear message. A copy of your work stays. | [ ] | Message text. |
| 6.17 | Save twice. Look at the folder. | `garden.kad.bak` shows the older file. | [ ] | Folder. |
| 6.18 | Make a big mesh: `python3 -c "..."` or export an OBJ with 300 000 triangles. A simple way: in Python run `doc.execute` to add a grid, or find a public OBJ. Import it in the modeler. | A message states the limit and counts (`mesh_too_large_for_surface`). The mesh stays a mesh. The program stays responsive. | [ ] | Message text, time, memory in Activity Monitor. |
| 6.19 | Try to draw past the surface limits: 50 000 vertices, 100 000 edges, 50 000 faces. A script is easier (`mesh.*` API). | The edit is refused with counts. Nothing is dropped silently. | [ ] | Message text. |
| 6.20 | Import `survey_rev1.dxf` 20 times into one drawing. | The program stays responsive. Memory stays under 2 GB. | [ ] | Memory, time. |
| 6.21 | Open a drawing with unsaved changes. File > New, then File > Open. | A question: Save and open, Don't save, Cancel. | [ ] | Dialog text. |
| 6.22 | Quit with unsaved changes. | The same three-button question. | [ ] | Dialog text. |
| 6.23 | Disconnect your network. Start the app. Look at Updates and the feedback queue. | No hang. Updates fail quietly. | [ ] | Delay at start. |
| 6.24 | Delete `prefs.json` while the app is shut. Start. Then put in `{ "bad": ` and start. | Defaults load. A warning shows for a damaged file. A copy of the damaged file stays. | [ ] | Message text. |

---

## Known gaps: do not report these

| Item | Status |
|---|---|
| Walk, Look Around, Position Camera in the modeler | Not built |
| Editor for keys in the modeler | Not built |
| Autosave and the agent server in the modeler | Not built (only in the app) |
| AGR-04 (the person points to the agent) | Planned |
| A tint of the surfaces in the 3D review (AGR-05) | Not built. A box shows. |
| The activity log survives a restart | Not built |
| Exact solids (MOD-14), true circles in DXF, STEP and IFC (MOD-16, MOD-17), true dimensions (MOD-18) | Planned |
| Batch print (SHT-07), design layer viewports (SHT-09), water budget (IRR-06) | Planned |
| Point cloud import, native `.vwx`, referenced DXF (IOP-12, IOP-13, IOP-25) | Planned |
| Number format in dimension text, worksheets, printed cells, schedules (PLAT-27) | Later |
| Web build: no agent port, no Python, no DXF or IFC import, no PDF | By design (`docs/web.md`) |
| Cmd/Ctrl+W, T, N inside a browser tab | The browser keeps them |

## How to report

Open an issue at <https://github.com/nemecec/kadakas-app/issues>. Use one issue per failure. Put the step number in the title, for example `Hand test 3.12: Paint Bucket paints both sides`.

### Where to find the data

| What | Where |
|---|---|
| Version | Help > About Kadakas (app and modeler) |
| Logs | The program writes to the terminal (stderr). Start from a terminal and copy the text. Run with `RUST_BACKTRACE=1` for a backtrace. `save-failed.log` is in the state folder. |
| Crash reports | `~/Library/Application Support/kadakas/launcher/crash-reports/<run id>.json` (Windows `%LOCALAPPDATA%\kadakas\launcher\crash-reports`, Linux `~/.local/state/kadakas/launcher/crash-reports`). The report holds the last 50 log lines, the last 40 commands and the GPU name. Read it before you share it. |
| Run records | `.../kadakas/launcher/runs/` |
| Settings | `.../kadakas/prefs.json`, `modeler.json`, `launcher/launcher.json` |
| Recovery copies | `.../kadakas/recovery` |
| GPU and OS | Apple menu > About This Mac > More Info. On Windows `dxdiag`. On Linux `glxinfo -B`. |

If a folder differs on your machine, run `find ~/Library/Application\ Support/kadakas -maxdepth 2` and write what you find.

### Bug template

```text
Title: Hand test <step number>: <short summary>

Version: (Help > About)
Program: kadakas-app / kadakas-modeler / launcher / kadakas CLI
How installed: build from source (commit ____) / dmg / MSI / NSIS / deb / AppImage
OS and version:
Machine and GPU: (for example MacBook Pro M1 Pro, Apple GPU)
Display: size, scale (percent), external monitor yes/no
Input: mouse / trackpad
Language, units, number format:
Settings changed from default:

Steps:
1.
2.
3.

Expected:
Actual:

How often: always / sometimes / once
Screenshot or screen recording: (attach)
File that triggers it: (attach, or say it holds private data)
Terminal output: (paste)
Crash report: (paste the text shown in the crash window, after you read it)
Anything else:
```

Do not paste the agent token from `agent.json`. Remove it before you share a file.
