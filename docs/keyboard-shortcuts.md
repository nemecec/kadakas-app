# Keyboard shortcuts

Cadektor defaults follow the Vectorworks 2026 defaults, so Vectorworks users find the same keys. Source of the
Vectorworks keys: https://app-help.vectorworks.net/2026/eng/assets/docs/VWKeyboardShortcuts.pdf

Rules
- `Cmd/Ctrl` means Cmd on macOS and Ctrl on Windows and Linux. `Alt` is Option on macOS.
- Status `done`: the key works in the app and is in the default keymap. The test `doc_matches_default_keymap`
  (`crates/cadektor-app/src/keymap.rs`) checks every `done` row that names an action id.
- Status `planned`: the feature is missing. The key is the Vectorworks default and will be used when the feature is built.
- Single-key shortcuts never fire while a text field has focus. Only Cmd/Ctrl chords do, except Undo, Redo and Select All,
  which stay with the text field.
- The first column also shows the action id used in the keymap file.

## Change the keys

Create `keymap.json` in the per-user config folder (or set `CADEKTOR_KEYMAP` to a file path):

| System | File |
|---|---|
| macOS | `~/Library/Application Support/cadektor/keymap.json` |
| Linux | `$XDG_CONFIG_HOME/cadektor/keymap.json`, else `~/.config/cadektor/keymap.json` |
| Windows | `%APPDATA%\cadektor\keymap.json` |

```json
{ "tool.line": "L", "edit.redo": ["Cmd/Ctrl+Y"], "edit.delete": [] }
```

A value is one chord or a list of chords. An empty list removes the key. A chord is modifiers joined by `+`
(`Cmd`, `Ctrl`, `Alt`, `Option`, `Shift`) and a key (`A`, `5`, `Tab`, `Escape`, `Backtick`, `[`, `Left`). If a chord
is already used by another action, the user file wins, the other action loses the key, and the status bar shows a warning.

## Deviations from the Vectorworks defaults

- Data Tag, North Arrow, Scale Bar, Revision Cloud and the Graphic Legend command have no Vectorworks default. They use `Alt+Shift+T`, `Alt+Shift+N`, `Alt+Shift+B`, `Alt+Shift+V` and `Alt+Shift+E`.
  The one Dimension tool covers the Vectorworks Constrained Linear, Angular and Radial dimension tools: `N`, `.` and `,` start it in the matching mode.
- Plant, Landscape Area and Hardscape have no Vectorworks default. They use `Alt+Shift+P`, `Alt+Shift+A` and `Alt+Shift+H`
  because the plain letters are taken (`P` is tool mode group 4, `H` is Pan).
- The first app version used `Cmd/Ctrl+Shift+Z` for Redo and `Home` for Zoom to Fit. Both stay as extra keys.
  Zoom to Fit moved from `Cmd/Ctrl+0` to `Cmd/Ctrl+6`, because `Cmd/Ctrl+0` is "Set 3D View" in Vectorworks.
- `Delete` and `Backspace` both take back the last clicked point while drawing (Vectorworks: Backspace on Windows, Delete on Mac), and delete the selection otherwise. Enter finishes polylines (Vectorworks: double-click).
- Modifiers: `Cmd/Ctrl` is the command key. `Ctrl` alone in a keymap file means the command key on Windows and Linux, but the real Control key on macOS. Use `Cmd` for the command key on macOS.
- The Data bar keys are not in the Vectorworks list. They follow its Data bar workflow (Tab to enter, Tab and Shift+Tab between fields).
- Space plus drag pans. Vectorworks lets you use any tool while Space is held.
- The 3D standard views use the Vectorworks numbering: 0 Top/Plan, 5 Top, 2 Front, 8 Back, 4 Left, 6 Right, 1 and 3 the front
  isometrics, 7 and 9 the rear isometrics. Vectorworks binds them to the numeric keypad, which cannot be changed there.
  egui cannot tell keypad digits from the digit row, and the plain digits are tool keys here, so Cadektor uses
  `Alt+Shift` plus the same digit. The digit is read from the physical key, because Shift changes the character on
  many layouts. Bottom has no key in Vectorworks and none here: use the view bar.
  Top/Plan is the 2D drawing. The other views are the 3D view. In 3D, drag orbits (the Flyover behavior), Shift-drag,
  Space-drag or middle-drag pans, and the wheel zooms toward the pointer. Zoom to Fit works in 3D too.

## Tools

Tool keys switch the active tool.

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| Selection tool (`tool.select`) | X (both) | X | done |
| Text tool (`tool.text`) | 1 (both) | 1 | done |
| Line tool (`tool.line`) | 2 (both) | 2 | done |
| Rectangle tool (`tool.rect`) | 4 (both) | 4 | done |
| Circle tool (`tool.circle`) | 6 (both) | 6 | done |
| Polyline tool (`tool.polyline`) | 5 (both) | 5 | done |
| Pan tool | H (both) | H | planned |
| Zoom tool | C (both) | C | planned |
| Arc tool | 3 (both) | 3 | planned |
| 2D Polygon tool | 8 (both) | 8 | planned |
| Reshape tool | - (hyphen) (both) | - (hyphen) | planned |
| Rotate tool | Option+= / Alt+= | Alt+= | planned |
| Mirror tool | = (both) | = | planned |
| Split tool | L (both) | L | planned |
| Trim tool | Option+Shift+L / Alt+Shift+L | Alt+Shift+L | planned |
| Fillet tool | 7 (both) | 7 | planned |
| Chamfer tool | Option+7 / Alt+7 | Alt+7 | planned |
| Offset tool | Shift+- (both) | Shift+- | planned |
| Clip tool | Option+C / Shift+N | Shift+N | planned |
| Constrained Linear Dimension tool (Cadektor: the Dimension tool with modes; `tool.dimension`) | N (both) | N | done |
| Angular Dimension tool (`tool.angular_dimension`) | . (both) | . | done |
| Radial Dimension tool (`tool.radial_dimension`) | , (both) | , | done |
| Callout tool (`tool.callout`) | Alt+1 (Windows) / Option+1 (Mac) | Alt+1 | done |
| Plant tool (Cadektor landscape tool) (`tool.plant`) | none | Alt+Shift+P (no VW default; Plant needs a free key) | done |
| Landscape Area tool (Cadektor) (`tool.landscape_area`) | none | Alt+Shift+A (no VW default) | done |
| Hardscape tool (Cadektor) (`tool.hardscape`) | none | Alt+Shift+H (no VW default; plain H stays Pan) | done |
| Site Modifiers tool (Cadektor; modes Pad and Grade Limits) (`tool.site_modifier`) | none | Alt+Shift+G (no VW default; Alt+Shift+D is the Door tool) | done |
| Data Tag tool (Cadektor) (`tool.data_tag`) | none | Alt+Shift+T (no VW default) | done |
| North Arrow tool (Cadektor) (`tool.north_arrow`) | none | Alt+Shift+N (no VW default) | done |
| Scale Bar tool (Cadektor) (`tool.scale_bar`) | none | Alt+Shift+B (no VW default) | done |
| Revision Cloud tool (Cadektor) (`tool.revision_cloud`) | none | Alt+Shift+V (no VW default; Alt+Shift+R is the Regular Polygon tool) | done |

## Annotation commands

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| Add a graphic legend (`cmd.graphic_legend`) | none | Alt+Shift+E (no VW default) | done |

## Tool modes

The Dimension tool has one group of mode buttons (Horizontal, Vertical, Aligned, Chain, Baseline, Ordinate, Angular, Radial, Diameter,
Arc length). Group 1 (`U`) picks the next mode. `.` and `,` start the tool in Angular and Radial mode.

Keys that cycle the groups of mode buttons in the tool bar (UI-11).
The Polyline, Landscape Area and Hardscape tools have one group of mode buttons: the vertex modes
(Corner, Bézier, Cubic, Tangent arc, Point on arc, Arc). Group 1 (`U`) picks the next vertex mode and wraps
around after the last one. The other groups are bound, but these tools have no buttons for them, so the key only
shows a message in the status line. Other tools have no mode buttons yet.

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| Tool mode group 1 (`mode.group1`) | U (both) | U | done |
| Tool mode group 2 (`mode.group2`) | I (both) | I | done |
| Tool mode group 3 (`mode.group3`) | O (both) | O | done |
| Tool mode group 4 (`mode.group4`) | P (both) | P | done |
| Tool mode group 5 (`mode.group5`) | [ (both) | [ | done |
| Tool mode group 6 (`mode.group6`) | ] (both) | ] | done |

## Snapping

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| Suspend snapping (hold) (`snap.suspend`) | ` (both) | Backtick (the back quote key) | done |
| Toggle SmartCursor cues | Y (both) | Y | planned |
| Toggle angles relative to prior segment | / (both) | / | planned |
| Snap loupe | Z (both) | Z | planned |
| Set smart point, edge or vector (snap lock) | T (both) | T | planned |
| Toggle Snap to Grid | none | - (menu View only; VW leaves it undefined) | done |
| Toggle Snap to Object | none | - (menu View only; VW leaves it undefined) | done |
| Toggle snap to angle, intersection, distance, smart points, smart edge, tangent | none | - | planned |

## View, zoom and pan

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| Zoom in, factor 2 (`view.zoom_in`) | Cmd+1 / Ctrl+1 | Cmd/Ctrl+1 | done |
| Zoom out, factor 2 (`view.zoom_out`) | Cmd+2 / Ctrl+2 | Cmd/Ctrl+2 | done |
| Zoom to fit objects (`view.zoom_fit`) | Cmd+6 / Ctrl+6 | Cmd/Ctrl+6 or Home (Home is an extra, kept from the first app version) | done |
| Zoom to normal scale | Cmd+3 / Ctrl+3 | Ctrl+3 | planned |
| Zoom to fit page area | Cmd+4 / Ctrl+4 | Ctrl+4 | planned |
| Pan left (`view.pan_left`) | Arrow (both) | Left | done |
| Pan right (`view.pan_right`) | Arrow (both) | Right | done |
| Pan up (`view.pan_up`) | Arrow (both) | Up | done |
| Pan down (`view.pan_down`) | Arrow (both) | Down | done |
| Pan while held, then back to the old tool (`view.pan_hold`) | Space (hold) (both) | Space (Space plus drag pans; VW lets you use any tool) | done |
| Top/Plan view, the 2D drawing (`view.top_plan`) | Num 0, and Cmd+5 / Ctrl+5 (Standard views) | Alt+Shift+0 or Cmd/Ctrl+5 | done |
| Top view, 3D (`view.top`) | Num 5 | Alt+Shift+5 | done |
| Front view (`view.front`) | Num 2 | Alt+Shift+2 | done |
| Back view (`view.back`) | Num 8 | Alt+Shift+8 | done |
| Left view (`view.left`) | Num 4 | Alt+Shift+4 | done |
| Right view (`view.right`) | Num 6 | Alt+Shift+6 | done |
| Left Isometric view (`view.left_isometric`) | Num 1 | Alt+Shift+1 | done |
| Right Isometric view (`view.right_isometric`) | Num 3 | Alt+Shift+3 | done |
| Left Rear Isometric view (`view.left_rear_isometric`) | Num 7 | Alt+Shift+7 | done |
| Right Rear Isometric view (`view.right_rear_isometric`) | Num 9 | Alt+Shift+9 | done |
| Flyover: orbit in 3D (`view.flyover`) | Shift+C (Flyover tool) | Shift+C | done |
| Wireframe rendering (`view.wireframe`) | Cmd+Shift+W / Ctrl+Shift+W | Cmd/Ctrl+Shift+W | done |
| Shaded rendering (`view.shaded`) | Cmd+Shift+G / Ctrl+Shift+G | Cmd/Ctrl+Shift+G | done |
| Toggle parallel and perspective projection (`view.projection`) | none | Alt+Shift+J | done |
| Next view in history | Cmd+Shift+. / Ctrl+Shift+. | Ctrl+Shift+. | planned |
| Previous view in history | Cmd+Shift+, / Ctrl+Shift+, | Ctrl+Shift+, | planned |
| Zoom in, factor 2 (double tap) | CC (both) | CC | planned |
| Switch saved view | Option+Arrow / Alt+Arrow | Alt+Arrow | planned |
| Switch active layer or class | Cmd+Arrow / Ctrl+Arrow | Ctrl+Arrow | planned |
| Class options (active only, gray, show, snap) | Cmd+Shift+Option+3 to 8 / Ctrl+Shift+Alt+3 to 8 | Ctrl+Shift+Alt+3 to 8 | planned |
| Layer options (active only, gray, show, snap) | Cmd+Option+3 to 8 / Ctrl+Alt+3 to 8 | Ctrl+Alt+3 to 8 | planned |

## Edit and menus

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| New (`file.new`) | Cmd+N / Ctrl+N | Cmd/Ctrl+N | done |
| Open (`file.open`) | Cmd+O / Ctrl+O | Cmd/Ctrl+O | done |
| Save (`file.save`) | Cmd+S / Ctrl+S | Cmd/Ctrl+S | done |
| Save As (`file.save_as`) | Cmd+Shift+S / Ctrl+Shift+S | Cmd/Ctrl+Shift+S | done |
| Print: exports the sheets as a PDF (`file.export_pdf`) | Cmd+P / Ctrl+P | Cmd/Ctrl+P | done |
| Undo (`edit.undo`) | Cmd+Z / Ctrl+Z | Cmd/Ctrl+Z | done |
| Redo (`edit.redo`) | Cmd+Y / Ctrl+Y | Cmd/Ctrl+Y or Cmd/Ctrl+Shift+Z (Shift+Z is an extra, kept from the first app version) | done |
| Select All (`edit.select_all`) | Cmd+A / Ctrl+A | Cmd/Ctrl+A | done |
| Cancel the current operation (`edit.cancel`) | Esc (both) | Escape (when idle, returns to the Selection tool) | done |
| Finish the object (polyline and polygons) (`edit.finish`) | none | Enter (in VW: double-click) | done |
| Delete the selection, or take back the last clicked point while drawing (`edit.delete`) | Delete / Backspace | Delete or Backspace (the VW list gives the key only for undoing the last point; deleting the selection with it is an extra) | done |
| Nudge objects left (one grid step) (`edit.nudge_left`) | Shift+Arrow (both) | Shift+Left | done |
| Nudge objects right (`edit.nudge_right`) | Shift+Arrow (both) | Shift+Right | done |
| Nudge objects up (`edit.nudge_up`) | Shift+Arrow (both) | Shift+Up | done |
| Nudge objects down (`edit.nudge_down`) | Shift+Arrow (both) | Shift+Down | done |
| Move objects by arrow | Shift+Cmd+Arrow / Shift+Ctrl+Arrow | Shift+Ctrl+Arrow | planned |
| Deselect all | XX (both) | XX | planned |
| Repeat last command | R (both) | R | planned |
| Cut / Copy / Paste | Cmd+X / Cmd+C / Cmd+V / Ctrl+X / Ctrl+C / Ctrl+V | Ctrl+X / Ctrl+C / Ctrl+V | planned |
| Paste in Place | Cmd+Option+V / Ctrl+Alt+V | Ctrl+Alt+V | planned |
| Duplicate | Cmd+D / Ctrl+D | Ctrl+D | planned |
| Duplicate Array | Cmd+Shift+Option+D / Ctrl+Shift+Alt+D | Ctrl+Shift+Alt+D | planned |
| Move (dialog) | Cmd+M / Ctrl+M | Ctrl+M | planned |
| Group (`edit.group`) | Cmd+G / Ctrl+G | Cmd/Ctrl+G | done |
| Ungroup (`edit.ungroup`) | Cmd+U / Ctrl+U | Cmd/Ctrl+U | done |
| Edit group (`edit.enter_group`) | Cmd+[ / Ctrl+[ | Cmd/Ctrl+[ | done |
| Exit group (`edit.exit_group`) | Cmd+] / Ctrl+] | Cmd/Ctrl+] | done |
| Rotate left 90 / right 90 | Cmd+L / Cmd+Shift+R / Ctrl+L / Ctrl+Shift+R | Ctrl+L / Ctrl+Shift+R | planned |
| Flip horizontal / vertical | Cmd+Shift+H / Cmd+Shift+V / Ctrl+Shift+H / Ctrl+Shift+V | Ctrl+Shift+H / Ctrl+Shift+V | planned |
| Trim (command) | Cmd+T / Ctrl+T | Ctrl+T | planned |
| Close polygon, polyline or path | K (both) | K | planned |
| Exit text editing | Esc (both) | Escape | planned |
| Palettes: Object Info, Navigation, Attributes, Resource Manager | Cmd+I / Cmd+Shift+N / Cmd+Shift+A / Cmd+R / Ctrl+I / Ctrl+Shift+N / Ctrl+Shift+A / Ctrl+R | Ctrl+I / Ctrl+Shift+N / Ctrl+Shift+A / Ctrl+R | planned |
| Quick search | F (both) | F | planned |
| Edit current workspace | Cmd+Shift+Option+W / Ctrl+Shift+Alt+W | Ctrl+Shift+Alt+W | planned |

## Data bar

Type exact values while drawing. Line and polyline use L (length) and A (angle). Rectangle uses W and H. Circle uses R. Before the first click all tools use X and Y. Empty fields follow the cursor. Enter creates the object through the normal command path.

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| Data bar: focus the first field, then next field (`databar.focus`) | none | Tab (Vectorworks lists no Data bar keys; Tab here follows its Data bar workflow) | done |
| Data bar: previous field (`databar.prev`) | none | Shift+Tab | done |
| Data bar: commit typed values | none | Enter (same key as Finish the object) | done |
| Data bar: back to the canvas | none | Escape (the drawing stays active) | done |

## Mouse modifiers

| Function | Vectorworks 2026 default (Mac / Windows) | Cadektor binding | Status |
|---|---|---|---|
| Pan the view | Wheel button drag (both) | Middle button drag | done |
| Zoom in and out | Wheel roll (both) | Wheel roll, trackpad pinch | done |
| Scroll the view | Ctrl-roll and Shift-roll (both) | Ctrl-roll and Shift-roll | planned |
| Toggle an object in the selection | none | Shift-click | done |
| Finish a polyline or polygon | none | Double-click | done |
| Duplicate an object while moving | Option-drag / Ctrl-drag | Option/Ctrl-drag | planned |
| Create a similar object | Cmd+Option-click / Ctrl+Alt-click | Cmd/Ctrl+Alt-click | planned |
| Activate the Selection tool | Double-click in the drawing (both) | Double-click in the drawing | planned |
| Previous or next view | Mouse buttons 4 and 5 (Windows only) | Mouse buttons 4 and 5 | planned |

## Not applicable

These Vectorworks shortcuts have no function in Cadektor and none is planned.

- Tools of other tool sets: Building Shell (Wall, Door, Window and joins), 3D Modeling, Visualization, Move Page,
  2D Locus, Symbol Insertion, Double Line, Rounded Rectangle, Oval, Quarter Arc, Freehand, Regular Polygon, Spiral, Eyedropper,
  Visibility, Attribute Mapping, Connect/Combine, Move by Points, Texture.
- Menus: Page Setup, Quit, Close, Multiple View Panes, the numeric keypad views (see the 3D keys above), render modes other than wireframe and shaded,
  Model menu (solids, extrusions, 3D power pack), AEC, Spotlight, Energos, Plug-in Manager, Origin, stacking order commands,
  Align and Distribute, Convert commands, Show and Hide Guides.
- Other keys: Toggle Automatic Working Plane, X-ray Select, Coincident Selection, Set Datum, Edit list row (F2), worksheet cell keys,
  Alt/Cmd-click to run the Selection tool inside another tool, Flyover with the wheel button (the wheel button pans).
- Keys reserved by the operating system (Cmd+H, Cmd+Shift+Q, Cmd+Shift+1 to 9, Alt+- and others).
