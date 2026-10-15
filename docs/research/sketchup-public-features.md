# SketchUp public feature research (for the Kadakas 3D-first modeler)

Scope: the desktop SketchUp 2026 line. The latest release found is SketchUp Desktop 2026.2 (released 2026-05-19). 2026.0 came out on 2025-10-07 (build 26.0.429 Windows, 26.0.428 Mac) and 2026.1 in between. The "Default Keyboard Shortcuts" help page was updated 2026-09-29. Sources are public web pages only (help.sketchup.com, the public SketchUp forum, public blogs). Nothing was downloaded or installed. No SketchUp code, extension, SDK or file was read. Text is paraphrased. Items marked (unverified) were not confirmed on a fetched page. Most "unverified" items come from general knowledge of the product, not from a page, so they must be checked against public docs or videos before they become a requirement.

Purpose: the Kadakas modeler (section 4.23, MOD-01 to MOD-13 of `docs/feature-spec.md`) must feel familiar to SketchUp users: same layout, tool logic, inference behavior, tool and concept names, default shortcuts, and icons with the same visual idea (we draw our own). Legal rules are in section 7 of the spec: no copied help text, no copied icons or screenshots, no SketchUp file format, and no SketchUp marks in the product name. Tool and concept names are used here as plain descriptive words. Legal review decides if the UI may use them (open point 1 in section 10).

Fetch note: help.sketchup.com pages are very long (about 150,000 characters, mostly navigation). The fetch tool read only the first 100,000 characters of each page, so a few details near the end of pages may be missing. Several of those are marked (unverified).

## 1. Window layout

Sources: https://help.sketchup.com/en/sketchup/user-interface ; https://help.sketchup.com/en/sketchup/toolbars ; https://help.sketchup.com/en/sketchup/managing-dialog-boxes-windows ; https://help.sketchup.com/en/sketchup-education/navigating-sketchup-schools-interface ; https://help.sketchup.com/en/release-notes/sketchup-desktop-20260

- Main areas: title bar (window buttons, model name, version), menu bar, toolbar(s), drawing area, status bar, and the tray with its panels.
- Menus (desktop): File, Edit, View, Camera, Draw, Tools, Window, Extensions, Help. On macOS the preferences are under SketchUp > Settings.
- Drawing area: the 3D view. It shows the three drawing axes. A simple human figure may be in the template, for scale.
- Default toolbar: Getting Started. A new model opens with it. It holds the first-steps tool set. The exact desktop order was not confirmed on an official page (unverified). The SketchUp for Web main toolbar order is confirmed: Select, Eraser, Paint Bucket, Pencil (Line), Rectangle, Push/Pull, Move, Rotate, Scale, Tape Measure, Orbit, Pan (https://help.sketchup.com/en/sketchup-web/sketchup-web-main-toolbar). The desktop Getting Started toolbar is close to this and groups related tools in drop-downs; Freehand is under the Line drop-down (confirmed). The other groupings (arcs, shapes) are general knowledge (unverified).
- Large Tool Set: an all-purpose toolbar. It combines the tools of the Principal, Drawing, Edit, Construction, Camera and Walkthrough toolbars in one strip (the exact set is from a third-party page, unverified). The Collaboration bar is also in the Large Tool Set (2026.0 notes).
- Other toolbars, all from View > Toolbars: Standard, Views, Principal, Drawing, Edit, Construction, Camera, Solid Tools, Sandbox, Shadows, Section, Styles, Warehouse, Dynamic Components, Tags, Add Location, Classifier, Measurements, Trimble Connect. On Windows, View > Toolbars opens a window to choose toolbars and has an Options tab for screen tips and large icons; a right-click on a toolbar also toggles toolbars. On macOS there is one toolbar at the top plus separate tool palettes (Large Tool Set, Add Location, Warehouse, Sandbox, Solid Tools, Advanced Camera Tools, Dynamic Components); it can be customized. Since 2026.1 a toolbar button is removed by a right-click inside the Toolbars menu.
- Status bar (bottom), four parts: geo-location pin (hover shows data, click opens the geo-location options); Model Info link (hover shows the "credits" of the selected entity); a tooltip area with usage and modifier-key help for the active tool; and the Measurements box. The Instructor question-mark icon in the status bar is from a third-party page (unverified).
- Measurements box: lower right. It shows live values while drawing (length, radius, angle, count). You type without clicking into it; Enter commits. After the operation ends, you can type a new value until you start another operation or change tool. A unit suffix overrides the template unit (`"` inch, `'` foot, `mm`, `cm`, `m`). Details per tool are in section 2. https://help.sketchup.com/en/using-measurements-box
- Tray (Windows): SketchUp provides a default tray with pre-selected dialogs on the right of the drawing area. A click on a panel title bar collapses or expands it (collapsed panels show as tabs); panels can be dragged up or down to reorder; a title bar drag floats a tray and a double-click pins it back. Window > Default Tray (hide, edit name and contents), Window > New Tray, Window > Manage Trays (create, rename, delete; the Default tray cannot be deleted). 2026.0 added a right-click on tray and panel headers to show or hide panels, and panels can be dragged between trays or out as a new tray (Windows). On macOS the same panels open as separate dialogs from the Window menu.
- Default tray panels. Confirmed: Materials, Styles, Tags (Windows default tray), and Soften Edges (Window > Default Tray > Soften Edges). The SketchUp for Schools page lists its panel set: Entity Info, Outliner, Instructor, 3D Warehouse, Materials, Styles, Tags, Shadows, Scenes, Display, Soften/Smooth, Model Info, Solid Inspector. The desktop list in the task (Entity Info, Materials, Components, Styles, Tags, Scenes, Shadows, Outliner, Soften Edges, Instructor) is plausible but the desktop order and the exact content of the default tray were not confirmed on a page (unverified).
  - Entity Info: properties of the selection. Confirmed items: Length box for edges, radius and sides for circles and polygons, solid status in the upper-left corner for groups and components, a tag dropdown, soft/smooth settings via the edge context menu.
  - Materials: opens when the Paint Bucket is activated. Has an Edit tab for color, opacity and textures. Since 2026.0 (Windows) the thumbnail type is selectable.
  - Components: In Model list and search. Since 2026.2 (Windows) the search filters the model first; 3D Warehouse is behind a button.
  - Styles: Select tab (choose, create), Edit tab (five setting groups, section 6), "Update Style with Changes".
  - Tags: eye icon per tag, tag folders, Details menu (Expand All, Collapse All, Select All, Delete Tag, Delete Tag Folder, Purge, Select All Tagged), Color by Tag.
  - Scenes: Add Scene, Update Scene, Show Details (name, description, saved properties, Include in Animation).
  - Shadows, Outliner, Soften Edges: section 6 and section 5.
  - Instructor: a hint pane for the active tool (a GIF and short steps, third-party description, unverified).
- Axes and default template: red is the right-left axis, green runs into the screen, blue is up (colors confirmed by the arrow-key lock tables: Right = red, Left = green, Up = blue). The origin is where the three axes meet. A model starts from a template that sets units, style and other defaults; several templates ship with the app. What the default template contains (figure, sky and ground, shadows off, front face white, back face blue-gray) is partly general knowledge (unverified).
- Search: Shift+S finds tools and commands (shortcut table). https://help.sketchup.com/en/sketchup/search-tools-commands-and-extensions
- SketchUp for Web differences that matter: no trays and no extensions; two toolsets ("Main" and "Expanded", which the user can edit); panels on the right collapse to icons; the status bar has Help, Language and Input (mouse or trackpad) selectors. Zoom Extents is Ctrl/Cmd+Shift+E, Zoom Window is Shift+W, Lasso is Shift+Space. https://help.sketchup.com/en/sketchup-web/sketchup-web-expanded-toolbar

## 2. Default tools

Common rules:
- Modifier pairs: Windows Ctrl = macOS Option for "copy / second behavior" keys; Windows Alt = macOS Command for "free / override" keys. This holds in every confirmed table below.
- Esc cancels the current operation.
- Measurements box separators: a comma between values (a semicolon where the locale uses a decimal comma, forum: https://forums.sketchup.com/t/precise-dimensions/158382).
- Coordinates (Line, Move): `[x,y,z]` absolute to the model origin; `<x,y,z>` relative to the start point.
- Shortcut letters are the same on Windows and macOS. Full table in section 3.

### Select (Space) and Lasso Select (Shift+Space)
Source: https://help.sketchup.com/en/sketchup/selecting-geometry
- Click, double click, triple click, window/crossing, modifiers: see section 7.
- Lasso: freehand drag (counter-clockwise includes only entities fully inside; clockwise includes everything the path touches), or click segment points and double-click to close.
- No Measurements box input.

### Eraser (E)
Sources: https://help.sketchup.com/en/sketchup/erasing-and-undoing ; https://help.sketchup.com/en/sketchup/softening-smoothing-and-hiding-geometry ; https://help.sketchup.com/en/sketchup/introducing-drawing-basics-and-concepts
- Click an edge: removes the edge and the faces it bounds. Drag across edges: they highlight and are erased on release. Faces cannot be erased directly with the tool (the context command Erase removes only the face).
- Shift+click: hide the edge. Ctrl (Option): soften and smooth the edges you erase. Alt (Command)+click: undo softening and smoothing.

### Line / Pencil (L)
Source: https://help.sketchup.com/en/sketchup/introducing-drawing-basics-and-concepts
- Click the start, click the end. Esc restarts. A line across a face splits it. A closed coplanar loop makes a face. The line color shows the axis it follows.
- Arrow keys lock an axis, Shift locks the current inference, Alt (Command) after the first click cycles three linear inference states (section 4).
- Measurements box: a length (`2.5`, `2.5m`), or coordinates `[3',5',7']` / `<1.5m,4m,2.75m>`. Retyping works until the next action.
- Edge editing: Move on an endpoint if the edge bounds no face; or the Length box in Entity Info.

### Freehand (no default key)
Source: https://help.sketchup.com/en/sketchup/drawing-freehand-shapes
- Press and drag, release to end. The result is a curve of many segments. Before the stroke, arrow keys lock the drawing plane (Left green, Up blue, Right red). Ctrl+minus (Command+minus) reduces segments; Ctrl+plus adds them back up to the original number. A stroke that ends at its start makes a closed shape. Move can edit the end point of a curve that bounds no face. No Measurements input (not mentioned, unverified).

### Arc group: Arc, 2 Point Arc (A), 3 Point Arc, Pie
Source: https://help.sketchup.com/en/sketchup/drawing-arcs
- Arc: click the center, click the start point (or type a radius), click the end (or type an angle). Open arc made of segments. Measurements: radius, then angle. After drawing: `10s` sets 10 segments; `20c` sets segments for a circle-based count.
- Pie: like Arc but makes a closed pie-shaped face.
- 2 Point Arc: click the start, click the chord end (or type a length), move perpendicular for the bulge (or type it). After drawing: a bulge value, a radius with `r` (`24r`), segments with `s` (`20s`). A half-circle inference appears; an arrow key locks it. A double click at later corners repeats the last parameters. Hover an edge first, then hold Alt (Command): the tangent inference locks.
- 3 Point Arc: click the start, click a pivot (shown as a dot), move and click the end. Tangent arcs work the same way.
- Default 12 segments per arc. Ctrl/Option + plus or minus changes the count. 2025.0 added filleting and unrounding for arcs.

### Rectangle (R) and Rotated Rectangle
Source: https://help.sketchup.com/en/sketchup/drawing-basic-shapes
- Rectangle: two diagonal corners. Measurements: `length,width` (`8',20'`). One value alone sets the length in the document unit; a leading comma changes only the width; a trailing comma only the length; a negative value flips the direction. Shift locks the golden-section inference. Square and golden-section inferences exist. Drawing from the center with Ctrl/Option is general knowledge (unverified).
- Rotated Rectangle (Draw > Shapes; no default key): an arrow key sets the plane; click the first corner; set the first edge by cursor or typed length; Shift keeps the first edge direction; Alt (Command) locks the protractor plane and sets its baseline; then set the angle (Shift constrains) and the width; click to finish.

### Circle (C) and Polygon
Source: same page.
- Circle: click the center, move out for the radius or type a radius with unit. The Measurements box first shows the number of sides, and a typed number changes it. Radius and sides can be changed afterwards in the box or in Entity Info. The default number of sides is not on the page (unverified; 24 from general knowledge). Arrow keys lock the plane.
- Polygon (no default key): the box shows the sides (minimum 3); type a number to change it, then the radius. Esc restarts. Entity Info edits radius and sides later.

### Push/Pull (P)
Source: https://help.sketchup.com/en/sketchup/pushing-and-pulling-shapes-3d
- Click a face, move, click. Works on any face. Pushing into the model cuts; a through-cut needs a parallel opposite face with no dividing lines (a limit message "offset is limited" shows at the opposite face).
- Measurements: a distance. The value stays in force until another tool is chosen. A double click on another face repeats the last distance. Ctrl (Option) then double click stacks an identical extrusion. A curved face extrudes into a surface of soft edges.
- A face extruded under about 1 inch can show edges through it (rendering limit). Esc restarts. Preselect a face to make a hard-to-hit face easier.

### Move (M)
Sources: https://help.sketchup.com/en/sketchup/moving-entities-around ; https://help.sketchup.com/en/using-measurements-box ; https://justnecessary.substack.com/p/the-sketchup-move-tool-decoded-auto
- Select, activate, click a reference, move, click. Shift (or an arrow key) locks the axis of the move line. Ctrl (Option) toggles copy (a + shows). Alt (Command) is auto-fold (releases the constraint that stops Move from folding faces; it adds extra faces and edges). On a group or component, grips show at the box corners; Alt (Windows) / Command (Mac) cycles corners, midpoints, side centers, and the center.
- Measurements: a distance (negative allowed), a unit value, `[x,y,z]`, `<x,y,z>`. After a copy: `5x` (five copies at the same offset each), `5*` (the "external" form on the official list with `x`), `5/` (copies divided evenly between the original and the first copy). The official page lists `3x`, `3*`, `3/` for the array syntax; the written form `/5` came from a search summary of help text (unverified on a fetched page). Retyping redraws the array until you click elsewhere.

### Rotate (Q)
Sources: https://help.sketchup.com/en/sketchup/flipping-mirroring-rotating-and-arrays ; https://help.sketchup.com/en/measuring-angles
- Select, activate. Hover to infer the plane, Shift locks it, Alt (Command) frees the protractor from the inferred plane. Click the center, click the start of the angle, click the end. Ctrl (Option) toggles copy. Gray corner grips on a group or component let you rotate it directly.
- Measurements: an angle in degrees (`34.1`) or a slope `rise:run` (`8:12`); negative is counter-clockwise. Precision 0.1 degree. Protractor ticks every 15 degrees near the center. Rotate arrays after a copy (`5x`, `/5`) are unverified.

### Scale (S)
Source: https://help.sketchup.com/en/sketchup/scaling-your-model-or-parts-your-model
- Corner grips scale uniformly (the box starts at 100%); edge and face grips stretch (starts at 1.00, labeled by axis such as "Red Scale"). Shift toggles uniform scaling. Ctrl (Option) scales about the center. Esc restarts.
- Measurements: a scale factor, a length with units, or comma-separated values for each axis. A component instance click scales that instance; opening the component first scales the definition. 2026.0: grips also place and size Live Components.

### Follow Me (no default key)
Source: https://help.sketchup.com/en/sketchup/modeling-specific-shapes-objects-and-building-features-3d
- Draw the profile on a plane at right angles to the path. Preselect the path with Select. Activate Follow Me and click the profile face. Delete the original path afterwards. The alternative of dragging the profile along the path edges, and Alt (Command) to use the face perimeter as the path, is general knowledge (unverified). No Measurements input.

### Offset (F)
Source: https://help.sketchup.com/en/sketchup/offsetting-line-existing-geometry
- Select two or more connected edges in one plane, or click a face. Click the source, move, click. Measurements: a distance, retype until the next change. A double click on another face repeats the same offset. Overlaps are removed by default; Alt (Command) keeps them. An offset arc is a non-editable curve, except arcs with three or more segments, which stay arcs.

### Tape Measure (T)
Sources: https://help.sketchup.com/en/measuring-distance ; https://help.sketchup.com/en/sketchup-desktop-20261
- Three modes, cycled with Ctrl (Option): Measure, Create Guide Lines (the default), Create Guide Points (new in 2026.1; a double click places one point). Measure: click start and end, the distance shows in the box; hovering an edge shows its length. Guides are dashed temporary lines that make no geometry. Typing a new value after a measure offers to resize the model (or a selected group). Esc cancels.

### Protractor (no default key)
Source: https://help.sketchup.com/en/measuring-angles
- Choose the plane by hover or arrow keys, Shift locks it, click the vertex, click the angle start, move and click. It measures or sets an angled guide line. Alt (Command) frees the plane. The Measurements box shows the angle and accepts a typed angle.

### Dimension (no default key)
Source: https://help.sketchup.com/en/sketchup/adding-text-labels-and-dimensions-model
- Click the start, move along the entity and click the end (or click a single line), pull out, click. Inferences work (endpoint, midpoint, on edge, intersection, arc or circle center). Types: linear, radius (arcs), diameter (circles); the context menu switches radius/diameter. A dimension stays tied to its entity. Typed text breaks the link (shown in red when "Highlight Non-Associated Dimensions" is on in Model Info). Entity Info edits text position (centered, outside start or end), color, font, size, alignment and end style (slash, dot, closed arrow, open arrow, none). Model Info > Dimensions updates all.

### Text and 3D Text (no default keys)
Source: same page.
- Text: screen text (click empty space, type, finish by clicking outside or Enter twice) or leader text (click an entity, move, click; it opens with the component name or face area). A double click on a face shows its area. Leader styles: Pushpin, View Based, Hidden. Arrow styles: None, Dot, Closed, Open. 2025.0 added dynamic values.
- 3D Text: a dialog (text, font, bold, alignment, height with units, Filled, Extruded value). Place, then Move positions it. A negative Extruded value makes engraving.

### Axes (no default key)
Source: https://help.sketchup.com/en/sketchup/adjusting-drawing-axes
- Click the new origin, click a point on the red axis, click a point on the green axis. A double click anywhere moves the axes there at once. Alt (Command) picks the blue or green axis to place. Face context menu: Align Axes. Axis context menu: Move... with exact values. Component: Change Axes.

### Section Plane (no default key)
Source: https://help.sketchup.com/en/sketchup/slicing-model-peer-inside
- Click a face to place, name and number it. Shift locks the orientation to the face under the cursor. Arrow keys set the normal: Up blue, Right red, Left green, Down parallel to the face. Edit with Move and Rotate. Context menu: Reverse, Align View, Active Cut, Create Group from Slice, Hide. A double click toggles Active Cut. Related commands: Display Section Planes, Display Section Cuts, Section Fill. File > Export > Section Slice (default DWG).

### Paint Bucket (B)
Source: https://help.sketchup.com/en/sketchup/applying-materials
- Opens the Materials panel. Alt (Command) samples a material from a face. The modes "Object Faces / Connected Faces / All Faces" are on the iPad tool page. The desktop modifiers Ctrl (fill connected faces) and Shift (fill all of the same material) are general knowledge (unverified).

### Navigation: Orbit (O), Pan (H), Zoom (Z), Zoom Window, Zoom Extents (Shift+Z), Position Camera, Look Around, Walk
Sources: https://help.sketchup.com/en/sketchup/viewing-model ; https://help.sketchup.com/en/sketchup/using-your-mouse ; https://help.sketchup.com/en/sketchup/walking-through-model
- Orbit: drag rotates around the center of the view; hold the middle button to orbit while another tool runs; a double click centers the model at the point; Ctrl (Option) rolls the camera; Shift pans.
- Pan: drag. Temporary pan: wheel plus left button; one-button/trackpad: Ctrl+Command+Shift.
- Zoom: drag up zooms in. The wheel zooms around the cursor; with the Zoom tool it zooms around the screen center. Shift+drag with Zoom (or Camera > Field of View) changes the field of view; type a value in degrees or in mm in the box.
- Zoom Window: drag a box. Zoom Extents fits the model. Camera > Previous / Next step through views. Standard views: Top, Bottom, Front, Back, Left, Right, Iso. Projection: Perspective (default), Parallel Projection, Two-Point Perspective.
- Position Camera: click a point to place the eye 5 ft 6 in above it; click-hold and drag to aim; type an eye height in the box. It then switches to Look Around.
- Look Around: drag to turn the head. Eye height is editable in the box.
- Walk: click and drag; the farther you drag, the faster you move; up/down moves forward/back. Shift moves up and down instead. Ctrl (Option) runs. Alt (both platforms, per the page) walks through walls (collision is on by default). Arrow-key walking: not on the page (unverified).

### Solid Tools (no default keys)
Sources: https://help.sketchup.com/en/sketchup/modeling-complex-3d-shapes-solid-tools ; https://help.sketchup.com/en/sketchup-ipad/solid-tools
- Outer Shell: keeps only the outer faces of overlapping solids. Union: merges solids (may keep inner geometry). Subtract: the first solid selected cuts the second and is deleted. Trim: like Subtract but the cutter stays. Intersect: keeps only the overlap. Split: splits overlapping solids along their intersection edges. Order matters only for Subtract and Trim.
- Access: Solid Tools toolbar, Tools > Outer Shell, Tools > Solid Tools, or preselect and use the context menu. Inputs must be solids (groups or components). No Measurements input.
- Plans: Outer Shell is in the free plan; the others need a paid plan. Intersect Faces > With Model is the free way to cut.

### Sandbox tools (no default keys)
Sources: https://help.sketchup.com/en/sketchup/sculpting-and-fine-tuning-terrain ; https://help.sketchup.com/en/sketchup/creating-terrain-scratch
- From Contours: select the contour lines, run it; the result is a TIN group. From Scratch: click a corner, set length and width by cursor or box; the grid spacing can be retyped. Smoove: select vertices, move up or down; the box takes a height and a radius. Add Detail: adds a vertex and its triangles; Shift constrains to horizontal; the box takes a height. Stamp: imprints an object's footprint; the box takes an offset (transition). Drape: projects edges onto a TIN. Flip Edge: flips a diagonal to remove flat spots.

### Tag tool (no default key)
Source: https://help.sketchup.com/en/sketchup/controlling-visibility-tags
- Click entities to apply the active tag. Alt (Command) samples a tag. Shift replaces the tag of all entities that share a tag. Ctrl (Option) tags all instances of a component.

### Flip (no default key)
Source: https://help.sketchup.com/en/sketchup/flipping-mirroring-rotating-and-arrays
- Select, activate; three half-transparent planes show; click one to flip about it. Arrow keys pick a plane (Left green, Right red, Up blue). Ctrl (Option) flips a copy. Alt (Command) toggles between the object's own axes and its parent's axes. A face under the cursor shows a magenta plane.

### Other default keys and tools
- Make Component (G), Image Igloo (I), Back Edges (K), Search (Shift+S) are in the official shortcut table.
- Product-specific tools out of scope for Kadakas: Add Location, Scan Essentials tools, Comment tool (2026.0), AI Render and AI Assistant (2026.1), Trimble Connect.

## 3. Default keyboard shortcuts

Sources: https://help.sketchup.com/en/default-keyboard-shortcuts (official, updated 2026-09-29; one key per tool, no separate Windows/macOS columns) ; https://help.sketchup.com/en/sketchup/selecting-geometry ; https://help.sketchup.com/en/sketchup/erasing-and-undoing ; https://help.sketchup.com/en/sketchup-web/sketchup-web-expanded-toolbar ; https://how-to-sketchup.com/sketchup-shortcuts (third-party, tool letters only).

| Command | Windows | macOS | Source |
|---|---|---|---|
| Select | Space | Space | official |
| Lasso Select | Shift+Space | Shift+Space | official |
| Paint Bucket | B | B | official |
| Eraser | E | E | official |
| Line | L | L | official |
| Rectangle | R | R | official |
| Circle | C | C | official |
| 2 Point Arc | A | A | official |
| Move | M | M | official |
| Push/Pull | P | P | official |
| Rotate | Q | Q | official |
| Scale | S | S | official |
| Offset | F | F | official |
| Tape Measure | T | T | official |
| Orbit | O (or middle-button drag) | O | official |
| Pan | H (or Shift+middle-button drag) | H | official |
| Zoom | Z (or wheel) | Z | official |
| Zoom Extents | Shift+Z | Shift+Z | official |
| Make Component | G | G | official |
| Image Igloo | I | I | official |
| Back Edges (toggle) | K | K | official |
| Search | Shift+S | Shift+S | official |
| Select All | Ctrl+A | Cmd+A | official |
| Deselect All | Ctrl+T | Shift+Cmd+A | official |
| Undo | Ctrl+Z (also Alt+Backspace) | Cmd+Z | official |
| Redo | Ctrl+Y | Shift+Cmd+Z | unverified |
| Cut, Copy, Paste | Ctrl+X, Ctrl+C, Ctrl+V | Cmd+X, Cmd+C, Cmd+V | unverified |
| New, Open, Save | Ctrl+N, Ctrl+O, Ctrl+S | Cmd+N, Cmd+O, Cmd+S | unverified |
| Delete selection | Delete | Delete | unverified |
| Zoom Window | Ctrl+Shift+W | Cmd+Shift+W | unverified (Web: Shift+W, official) |
| Zoom Extents (Web) | Ctrl+Shift+E | Cmd+Shift+E | official (Web only) |
| Hide, Unhide, Make Group, Explode, Intersect Faces, Standard views | none by default | none by default | unverified |

Other points:
- Tools with no default shortcut on the official table: Polygon, Rotated Rectangle, 3 Point Arc, Pie, Freehand, Follow Me, Protractor, Dimension, Text, 3D Text, Axes, Section Plane, Position Camera, Look Around, Walk, Flip, Solid Tools, Sandbox tools, Tag tool. (The Web expanded toolset also lists none for these.)
- Shortcuts can be changed in Window > Preferences > Shortcuts (Windows) or SketchUp > Settings > Shortcuts (macOS). https://help.sketchup.com/en/creating-and-editing-shortcuts
- During a drawing tool: arrow keys lock axes, Shift locks the inference, Ctrl/Option+plus and minus change arc and freehand segments (section 4).

## 4. Inference engine

Sources: https://help.sketchup.com/en/sketchup/introducing-drawing-basics-and-concepts ; https://forums.sketchup.com/t/line-tool-inferencing-control-in-sketchup-2020-2-what-do-you-think/133261 ; https://mastersketchup.com/sketchup-inference/ (third-party, colors).

Point inferences. Each has a color and a ScreenTip. Official list: Origin point, Component Origin Point, Endpoint, Midpoint, Arc Midpoint, Intersection, On Face, On Edge, Center (circle, arc, polygon), Guide Point, On Line (along a guide), On Section, Intersection with Hidden Section. The help page does not list the colors, except that all point inferences turn magenta inside a group or component. Third-party colors (unverified): green dot = endpoint; light blue/cyan dot = midpoint; red square = on edge; black = intersection; a blue square for on face is general knowledge. ScreenTips use the names above ("Endpoint", "Midpoint", "On Edge", "On Face", "Intersection", "Center").

Linear inferences:
- On Red / Green / Blue Axis: the line takes the axis color; a temporary dotted line may show. ScreenTip "On Red Axis", etc.
- From Point: a dotted line in the axis color from a point you paused on.
- Through Point: pause on a second point, then hold Shift to lock the direction from the start point through it.
- Parallel to edge, Perpendicular to edge, Perpendicular to face: magenta.
- Extend Edge: continues an edge. Tangent at Vertex: an arc tangent to the previous arc.
- Shape inferences: Square, Golden Section, Half/Quarter/Three-Quarter Circle, Arc side and center.

Locking:
- Shift locks the current inference. Pause on the wanted inference, press Shift, then move on.
- Arrow keys: Up = blue axis, Left = green, Right = red, Down = parallel or perpendicular (magenta). (The same keys pick the plane in Freehand, Section Plane, Flip, Rotated Rectangle.)
- Shift+Alt (Windows) / Shift+Command (macOS): locks the plane and the inferred face plane.
- Rotate and Protractor: Alt/Command frees the tool so the center moves but the drawing plane stays.
- Circle and Rotate can lock a plane by the colored direction of the normal.

Cycling:
- Linear inferencing toggle: after the first click of the Line tool, Alt (Windows) or Command (macOS) cycles three states: all inferences on (default), all off (point inferences still work), parallel and perpendicular only. The state resets when you pick another tool (forum thread).
- Pausing on a point makes the engine prefer alignment to it.
- Alt/Command also cycles grip types on a group or component in Move and Rotate.
- A "Tab" cycle was not found on any page (unverified; do not assume).
- Cues: the "Display cross hairs" preference shows axis-colored crosshairs; Styles > Edge Settings > Color "By axis" colors edges by axis.
- 2025.0 improved Snaps (Connected by Snaps, parallel lock). Details were not fetched (unverified): https://help.sketchup.com/en/sketchup/snaps

## 5. Geometry model

Sources: https://help.sketchup.com/en/sketchup/introducing-drawing-basics-and-concepts ; https://help.sketchup.com/en/sketchup/components ; https://help.sketchup.com/en/sketchup/creating-basic-component (via search summary) ; https://help.sketchup.com/en/context-menu-items-entities ; https://help.sketchup.com/en/sketchup/softening-smoothing-and-hiding-geometry ; https://help.sketchup.com/en/3d-warehouse/orienting-faces (via search summary) ; https://help.sketchup.com/en/sketchup/modeling-complex-3d-shapes-solid-tools

- Edges and faces: edges are the base. Closed coplanar loops of edges become faces. Rectangle, Circle, Polygon and the arc group make edges and faces directly.
- Face creation and splitting: a closed loop gets a face automatically. An edge drawn across a face splits it. Erasing the splitting edge merges the faces again. Push/Pull can then move only one part.
- Erasing: the Eraser on an edge removes the faces it bounds. Context Erase on a face removes only the face. To get a face back, redraw one of its edges.
- Sticky geometry: ungrouped geometry that touches other ungrouped geometry merges with it and moves with it. The official Drawing Basics page does not describe it. Forum answers confirm it: raw edges and faces stick, Move drags connected geometry, grouping or componentizing isolates geometry, tags do not separate it, and exploding a group can merge geometry again (https://forums.sketchup.com/t/why-are-my-objects-getting-attached-to-each-other/11661). Triple click selects all connected raw geometry.
- Group: a container that keeps its geometry apart from the rest. Component: a definition plus instances. Edits of geometry inside one instance change the definition and all instances; move, rotate, scale and flip of one instance do not.
  - Edit context: double click or context menu "Edit Group / Edit Component". Click outside to close. Point inferences inside the context are magenta.
  - Make Unique: gives the instance its own definition (the name gets a "#1"-like suffix). Explode: back to raw geometry; for a component it purges the definition. Other component menu items: Reload, Save As, Lock/Unlock, Reset Scale, Reset Skew, Scale Definition, Change Axes, Intersect Faces, Unglue.
  - Create Component dialog: Glue To (Any, Horizontal, Vertical, Sloped), Cut Opening (the component cuts a hole in the face it is glued to), Replace Selection with Component (on by default), axes options. Always Face Camera is a related option (unverified on a fetched page).
  - Components can be replaced for all instances. Dynamic Components and Live Components are configurable library items; Dynamic Components are not on Web.
- Soft, smooth and hidden edges: a soft edge is hidden and the faces it separates become one surface (selecting one face selects the whole surface). A smooth edge adds shading only and stays visible. A hidden edge is invisible and does not merge faces. Soften Edges panel: angle slider (largest angle to soften), Smooth normals (on by default), Soften coplanar (removes edges between coplanar faces). View > Hidden Geometry / Hidden Objects shows hidden items ghosted; Unhide in the context menu. Extruded arcs, circles and curves get soft and smooth edges by default. Entity Info shows the flags of an edge.
- Front and back faces: each face has two sides with their own color or material. Fronts should point outward. Context menu: Reverse Faces (selected faces), Orient Faces (make adjacent faces agree with the clicked face). The Monochrome face style helps to see sides. Default colors (white front, blue-gray back) are general knowledge (unverified).
- Solids: a group or component is a solid when it has a finite closed volume with no holes and no faces that do not meet at an edge. Entity Info shows solid status. Solid Tools accept only solids. Solid Inspector fixes reversed faces and stray edges and flags nested groups and short edges (extension on desktop, panel on Web for paid plans).
- Intersect Faces (context menu): With Model, With Selection (the help shows "With Selected"), With Context. It makes new edges where shapes meet and then you erase the unwanted parts. It works on non-solids.
- Curves (arcs, circles, polygons, freehand) are multi-segment entities that act as a unit; Explode Curve breaks them. Guides are not geometry.
- Other entities: images, text, dimensions, section planes, guides.

## 6. Organization

Sources: https://help.sketchup.com/en/sketchup/controlling-visibility-tags ; https://help.sketchup.com/en/sketchup/working-hierarchies-outliner ; https://help.sketchup.com/en/sketchup/creating-scenes ; https://help.sketchup.com/en/sketchup/creating-and-editing-style ; https://help.sketchup.com/en/sketchup/casting-real-world-shadows (via search summary) ; https://help.sketchup.com/en/sketchup/letting-fog-roll-your-model ; https://help.sketchup.com/en/sketchup/slicing-model-peer-inside

- Tags (renamed from Layers; same function and more): new geometry is "Untagged". Assign with the Tag tool or the Tag dropdown in Entity Info. The help advises tagging groups and components, not edges or faces, and leaving the active tag alone. The eye icon toggles visibility; Ctrl/Command multi-select toggles several tags or folders. Tag folders group tags; hiding a folder hides its tags. Color by Tag and a dash pattern per tag exist. Tag visibility does not stop geometry from merging.
- Outliner: a tree of groups, components and section planes. Expand one or all; sort by name; search filter; rename groups (triple click or context menu; component names change through the definition); drag to move in the hierarchy; the eye icon hides an item and all inside it; hidden items show hatched when selected; icons show open, closed, locked, hidden, visible, inactive.
- Scenes: a scene stores view and display state. Saved properties (checkboxes in the scene details): camera (location, zoom, field of view; the camera mode is also kept), hidden geometry and objects (top level), visible tags, active section planes, style and fog, shadow settings (time and date), environment (sun position, skybox), axes location and visibility, point cloud visibility (Scan Essentials). Add Scene, Update Scene, Update Scene Thumbnail, scene tabs along the top of the view (Move Left/Right), Include in Animation, name and description. Apply Tags to Scenes (2025.0) copies tag visibility to selected scenes. Scene add, delete and edit are undoable since 2026.0. Transition time and scene delay are in Model Info > Animation (unverified, not on the fetched page).
- Styles: five groups on the Edit tab. Edge Settings (Edges, Back Edges, Profiles, Depth Cue, Extension, Endpoints, Jitter, color by material, by axis or all the same), Face Settings (front and back colors; styles Wireframe, Hidden Line, Shaded, Shaded With Textures, Monochrome, X-ray, Photoreal Materials (2025.0), Ambient Occlusion (2024.0), transparency), Background Settings (sky, horizon, ground), Watermark Settings (image behind or over the model; jpg, png, psd, tif, tga, bmp; stretch, tile or position), Modeling Settings (colors of guides, axes, section planes, section cut width, section fill, hidden geometry). The list of edge options comes from the iPad and Web pages and may differ slightly on desktop. "Update Style with Changes" saves. Styles live in the model; collections hold styles.
- Shadows: toggle; time and date sliders; time zone; Light and Dark sliders; Use Sun for Shading; On Ground; On Faces; From Edges. The model needs a geolocation. An environment's Set Sun Location overrides the panel. 2026.2 adds a fast shadow system (can be turned off in preferences, Windows) and the Analysis Hub with Advanced Shadows.
- Fog: Display Fog; two distance sliders (where fog starts, where it reaches 100%), relative to the current view; the color is the background color or a default gray.
- Section planes: see section 2. Many planes can exist; one is the active cut per context (each group or component is a context). Section Fill needs a closed loop; "Troubleshoot Section Fill" marks gaps with red circles. Section fill settings belong to the style.

## 7. Selection behavior

Source: https://help.sketchup.com/en/sketchup/selecting-geometry ; https://help.sketchup.com/en/context-menu-items-entities

- Click: one entity (edge, face, group, component). Click empty space: clear.
- Double click: on a face selects the face and its bounding edges; on an edge selects the edge and its connected faces; on a group or component opens it for editing.
- Triple click: all connected entities.
- Window select (drag left to right): only entities fully inside the box. Crossing select (drag right to left): entities inside or touched by the box. The rectangle line style (solid or dashed) is unverified.
- Lasso (Shift+Space): see section 2.
- Add: Ctrl (Option). Toggle: Shift. Remove: Ctrl+Shift (Option+Shift). The cursor shows + or -.
- Select All: Ctrl+A / Cmd+A. Deselect All: Ctrl+T / Shift+Cmd+A.
- Context menu with edges selected: Connected Faces, All Connected, All with Same Tag, Deselect Edges, Invert Selection. With faces and edges: Bounding Edges, All with Same Material, Deselect Faces. A right click on empty space shows Select All (since 2026.1). Selection by tag and by material exists only through these menus.
- Per-entity context menus (all entities: Entity Info, Erase, Make Component/Group, Hide, Save Out, Reload, Zoom Selection). Group: Edit Group, Explode, Intersect Faces, Lock, Make Component, Reset Scale, Reset Skew, Soften/Smooth Edges, Unglue. Component: Edit Component, Explode, Intersect Faces, Lock, Make Unique, Change Axes, Reload, Save As, Scale Definition, Reset Scale/Skew, Unglue, Dynamic Components, 3D Warehouse. Face: Align View, Align Axes, Area, Combine Textures, Intersect Faces, Make Unique Texture, Modify Face, Orient Faces, Reverse Faces. Section Plane: Active Cut, Align View, Create Group from Slice, Reverse. Edge or curve: Divide, Select, Soften, Explode Curve, Find Center, Convert to Polygon.
- Tip from the help: turn a selection into a group to reselect it later.

## 8. Icon ideas (our own drawing; no images are linked or embedded)

Each entry is a visual idea only. We draw original art with different shapes, line weights and colors.

| Tool | Idea |
|---|---|
| Select | An arrow cursor pointing up-left |
| Lasso Select | An arrow cursor inside a loose loop |
| Eraser | A tilted block eraser with a small rubbed trail |
| Paint Bucket | A tipping bucket pouring a drop of color |
| Line / Pencil | A pencil with a short line at its tip |
| Freehand | A pencil with a wavy curve |
| Arc, 2 Point Arc, 3 Point Arc | A curve with dots at its ends; 3 Point adds a third dot |
| Pie | A wedge with a curved edge and two straight sides |
| Rectangle | A tilted rectangle with a corner dot; Rotated Rectangle is a skewed rectangle with an angle mark |
| Circle | An ellipse in perspective with a center dot |
| Polygon | A hexagon in perspective |
| Push/Pull | A box with an arrow rising from its top face |
| Move | A cross of four arrows |
| Rotate | A circular arrow around a pivot dot |
| Scale | A small square inside a larger square with a diagonal arrow |
| Follow Me | A profile on a path with an arrow along the path |
| Offset | Two nested outlines |
| Tape Measure | A tape measure case with an extended tape |
| Protractor | A half circle with ticks and a pointer line |
| Dimension | A line with arrow ends, a number in the middle, and two short extension lines |
| Text | A capital A in a frame with a leader line |
| 3D Text | A thick capital letter with depth |
| Axes | Three lines (red, green, blue) meeting at a point |
| Section Plane | A translucent plane cutting through a box |
| Orbit | Two curved arrows around a ball |
| Pan | An open hand |
| Zoom | A magnifying glass with a plus |
| Zoom Window | A magnifying glass over a dashed rectangle |
| Zoom Extents | A box with four outward corner arrows |
| Position Camera | A person figure on a ground point with a sight line |
| Look Around | An eye on a pivot with a left-right arrow |
| Walk | Two footprints |
| Outer Shell | Two overlapping cubes, only the outer outline filled |
| Intersect | Two overlapping circles, only the overlap filled |
| Union | Two overlapping circles, all filled |
| Subtract | Two overlapping circles, the first minus the overlap |
| Trim | As Subtract, with the cutter kept as an outline |
| Split | Two overlapping circles shown as three separate pieces |
| Sandbox: From Contours | Stacked contour lines |
| Sandbox: From Scratch | A flat grid |
| Sandbox: Smoove | A grid with a raised bump and a hand |
| Sandbox: Stamp | A footprint pressed into a grid |
| Sandbox: Drape | A sheet over a hill |
| Sandbox: Add Detail | A grid with an extra vertex dot |
| Sandbox: Flip Edge | A quad with a turning diagonal |
| Tag | A price-tag shape |
| Flip | A shape and its mirror image about a dashed plane |
| Make Component | A small block icon with a frame |

## 9. What is new in 2024 to 2026, and plan differences

Sources: https://help.sketchup.com/en/release-notes/sketchup-desktop-20260 ; https://help.sketchup.com/en/sketchup-desktop-20261 ; https://help.sketchup.com/en/sketchup-desktop-20262 ; https://help.sketchup.com/en/release-notes/sketchup-desktop-20250 ; https://help.sketchup.com/en/release-notes/sketchup-desktop-20240 (search summary only) ; https://www.cgchannel.com/2024/04/trimble-releases-sketchup-2024-0/

- 2026.0 (2025-10-07): Collaboration bar (share by invite or link; viewers navigate, measure, comment) and a Comment tool; Ambient Occlusion distance multiplier and color; roughness map invert; selectable material thumbnails (Windows); lower memory and faster selection, scene transitions, Zoom Extents and tag purge; Scale grips for Live Components; Paint Bucket applies materials beyond a component's presets; scale grips shown through geometry; rotation grips hidden when the plane is locked; place objects by a profile edge; scene updates undoable; Purge Unused off by default and in the Edit menu; DWG import options (layers as groups, flattened linework, hatches, section planes); IFC 2x3 and IFC 4 merged into one IFC option; Scan Essentials tools (Texture Projection, Surface Mesh, Scene Manager).
- 2026.1: SketchUp AI (AI Render from the viewport; AI Assistant with Help and Generate Object; monthly AI credits Free 0, Go 100, Pro 150, Studio 200); Migrate Preferences; Tape Measure guide points; single-page PDF import (Windows); right-click on empty space shows Select All.
- 2026.2 (2026-05-19): Analysis Hub with Advanced Shadows; Migrate Extensions without restart; point cloud visibility in Scene Properties (Windows); Components panel search filters the model; new LayOut graphics engine as default; fast shadow system; Revit Importer update.
- 2025.0: Environments panel (HDRI/EXR), Photoreal Materials (PBR), Generate Textures (AI), Trimble Connect import/reload/Save Out, Apply Tags to Scenes, Extension Migrator, purge reminder on save, improved Rotate (grips, protractor), Arc fillet/unround, Snaps (Connected by Snaps, parallel lock), dynamic values in Text, faster IFC import; Style Builder removed.
- 2024.0: new graphics engine, Ambient Occlusion face style, Trimble Connect integration, Add Location rework with True North arrow, faster IFC import and IFC 4 export, USDZ and glTF import and export, Scan Essentials Ground Mesh, extension error dialog.
- Plans (third-party price guides only; sketchup.com was not fetched, so unverified in detail): Free (Web, limited), Go (web and iPad, no desktop), Pro (desktop, LayOut, extensions, DWG/DXF/IFC), Studio (Pro plus V-Ray, Scan Essentials and Revit Importer, Windows only). Prices differ between guides and are not repeated. The 2026.0 notes name no "Pro only" feature; they only say Scan Essentials and browsing large Live Components need a subscription.
- Web (official table, https://help.sketchup.com/en/sketchup-web/web-features): free users get the Components panel, Tags, view-only Styles and Materials, basic geolocation, shortcuts, Live Components, STL-only export and only the Outer Shell solid tool. Paid users add Style and Material editing, the Outliner, import and full export, tag folders, all Solid Tools, and Site Context. Not on Web: extensions, Dynamic Components, high-resolution printing. The page does not mention Scenes or LayOut.
- Desktop-only compared with Web (from the sources above): trays and panel layouts, Extension Warehouse, LayOut, Scan Essentials, Photoreal Materials. Whether a free desktop use exists was not found (unverified).

## 10. Mapping to Kadakas

Rows MOD-01 to MOD-13 are in section 4.23 of `docs/feature-spec.md`.

| SketchUp concept | Kadakas concept | Gap or note (MOD row) |
|---|---|---|
| Tag (was Layer), tag folder | Class (and layer), shown as a tag | MOD-05 shows visibility by class and layer "as tags". Missing: tag folders, the Tag tool and its modifiers, Color by Tag, Purge. |
| Scene | Saved view (camera, visible classes and layers, render style) | MOD-10. Missing: scene tabs, per-scene property checkboxes, Update Scene, animation, Apply Tags to Scenes, and scene data for section planes, shadows and fog. |
| Style (edge and face settings) | Render style (3D-08) | MOD-10. Missing: profiles, jitter, depth cue, watermark, edge color by axis, Modeling Settings, collections. |
| Component (definition, instances) | Symbol | MOD-07 has edit in place, make unique, explode. Missing: Glue To, Cut Opening, Reload/Save As, Always Face Camera, Live and Dynamic Components. |
| Group | Group | MOD-07. |
| Edge, face, sticky raw geometry | Surface model: vertices, edges, faces with holes | MOD-06 covers face creation, splitting, merging when an edge is erased, and "geometry in one container sticks together". Hidden edges and Hide/Unhide are not named in MOD rows. |
| Soft and smooth edges, Soften Edges panel | Soft and smooth edges | MOD-08. The panel (angle slider, Smooth normals, Soften coplanar) is missing. |
| Front and back faces, Reverse Faces, Orient Faces | none | Missing. |
| Material, Paint Bucket | Material (3D-06) | MOD-05 lists paint. Missing: Sample, fill modes, texture positioning. |
| Solid group, Solid Tools | Closed surface model, boolean tools | MOD-09 covers union, subtract, intersect, trim, split and the closed-solid check. Outer Shell is missing. |
| Intersect Faces | Intersect faces | MOD-08. |
| Push/Pull, Follow Me, Offset | Same tools | MOD-05 (objects with a height) and MOD-08 (any planar face). Missing: Ctrl "new face", repeat by double click, Move auto-fold. |
| Line, Rectangle, Circle, Polygon, Arc | Same names | MOD-08. Rotated Rectangle, 2 Point Arc, 3 Point Arc and Pie are not named. Freehand and Lasso are missing. |
| Measurements box (`5x`, `/5`, `[x,y,z]`, `12s`, `24r`, `8:12`) | Measurements box | MOD-04 lists `2.5`, `3;4`, `5x`, `/5`. Missing: absolute and relative coordinates, `s`/`r`/`c` suffixes, slope `a:b`, the `*` form, unit suffixes. |
| Inference engine | 3D snaps | MOD-03 lists endpoint, midpoint, edge, face, center, intersection, axis lock, parallel, perpendicular, from point, Shift and arrow keys, a label. Missing: tangent, extend edge, through point, square and golden section, half circle, on-section, the Alt/Command three-state toggle, magenta color in groups, Origin and Component Origin. |
| Tape Measure, guides, Protractor | Tape measure and guides | MOD-05. Missing: guide points, stand-alone Protractor, rescale model by measuring, guide delete. |
| Dimension, Text, 3D Text | Annotation (section 4.19) | Dimensions and text exist in the app (ANN rows) but not as 3D entities in the modeler. 3D Text is missing. |
| Axes tool | User origin, working plane | Missing in the modeler. |
| Section Plane, Active Cut, Section Fill | Section viewport and cut (section 4.20) | MOD-13 puts surface models in section drawings. The live section plane in the 3D view, Active Cut and fill are missing. |
| Shadows, Fog, Environments | Render settings | Not in MOD rows. Missing. |
| Orbit, Pan, Zoom, Zoom Window, Zoom Extents, standard views, perspective and parallel, field of view | Same | MOD-02. Position Camera, Look Around, Walk are "later". Two-Point Perspective and Previous/Next view are not named. |
| Entity Info | Object Info palette | MOD-01 (tray). |
| Outliner | none | Missing. |
| Instructor | none | Missing (optional). |
| Components panel, 3D Warehouse | Resource Manager (symbols) | An online warehouse is out of scope. |
| Sandbox tools (TIN) | Site model (Vectorworks-like app) | Not in MOD rows. Landscape work comes later. |
| Extensions, Ruby API | Python, MCP, `describe` | MOD-11. No Ruby. |
| .skp format | none | MOD-12 states it is neither read nor written. |
| Default shortcuts | "SketchUp-like" profile | MOD-01. Section 3 is the base. |

Open points for the spec owner:
1. Legal review of using the tool and panel names ("Push/Pull", "Follow Me", "Tape Measure", "Outliner", "Scenes", "Tags", "Entity Info") in the UI. The aim is a familiar feel; spec section 7 forbids marks in the product name and marketing.
2. Parts of the shortcut table (menu commands) are unverified. Confirm them from public docs or public videos before the profile is frozen.
3. Default segment counts (SketchUp arcs: 12; Circle default not confirmed on a page) should come from a benchmark chosen with the user, as the measured-limits rule asks.
