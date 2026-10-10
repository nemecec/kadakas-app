# Cadektor Feature Spec

Cadektor is a clean-room, open-source CAD/BIM desktop app. A web build is a bonus. Its workflow and concepts match the latest Vectorworks (2026), so Vectorworks users feel at home. Landscape design is the first market.

Source of truth for Vectorworks behavior: `docs/research/vectorworks-public-features.md` (public pages only).

Priority: P0 = MVP, P1 = v1, P2 = later. Status: done = exists in `crates/cadektor-core`, planned = not built.

## 1. Goals and non-goals

Goals
- Let a Vectorworks landscape designer do real work with few surprises.
- Use the same concepts and terms: layers, classes, viewports, styles, record formats.
- Exchange data through open formats: IFC, DXF, GIS.
- Make every change scriptable and testable. AI agents are first-class users.
- Keep the core free of UI code. It runs in the desktop app, in the browser (WebAssembly), and headless.

Non-goals
- No copy of Vectorworks code, icons, help text, or exact UI art.
- No use of Vectorworks trademarks.
- No native `.vwx` support until legal review ends.
- No entertainment, lighting, or mechanical-engineering tools at first.
- No cloud service, accounts, or online project server in MVP.

## 2. Decisions

1. Stack: Rust, egui/eframe, wgpu. The core crate has no UI dependencies and compiles to WebAssembly.
2. Interop first: IFC (2x3, 4, 4x3), DXF, GIS (Shapefile, GeoJSON, georeferencing with WKT and EPSG). DWG comes later.
3. Native `.vwx` is deferred pending legal review.
4. Terminology matches Vectorworks. Terms include design layer, sheet layer, class, viewport, Object Info palette, and Resource Manager. Others include record format, worksheet, data tag, site model, plant style, landscape area, and hardscape. Trademarks, icons, help text, and exact UI art are never used.
5. AI-agent control is first-class:
   - Every change is a serializable, undoable `Command`.
   - JSON-RPC 2.0 API over stdio (`cadektor serve`).
   - Same API over TCP to the live app (`--agent-port` or `CADEKTOR_AGENT_PORT`). The app writes a random token and its port to a runtime file that only the current user can read. The first message on a connection must be `auth` with that token.
   - MCP server (`cadektor mcp`) for Claude Code.
   - SVG render so agents can see results.
   - A `describe` method for discovery.
   - Planned: PNG render, live viewport screenshot, Python scripting.
6. Testing: Rust unit and property tests. Python behavior tests over the JSON-RPC API. Core line coverage of 85% or more (cargo-llvm-cov). CI on macOS, Linux, Windows.
7. License: Apache-2.0 OR MIT.

## 3. Concept glossary

| Term | Meaning in Cadektor |
|---|---|
| Document | One project file. Holds layers, classes, objects, resources, settings. |
| Design layer | Holds the model. 2D and 3D share one space. Has scale, elevation, lock, visibility. |
| Sheet layer | Holds a printable sheet with viewports and annotations. |
| Class | A named attribute and visibility group. A second axis next to layers. |
| Viewport | A view of design layers on a sheet layer. Has scale, render style, class/layer overrides. |
| Saved view | A stored set of layer and class visibility, camera, and phase. |
| Navigation palette | One palette for classes, layers, sheets, viewports, saved views. |
| Object Info palette | Shows and edits the selected object. Tabs: Shape, Data, Render. |
| Resource Manager | Browser for reusable resources: symbols, styles, hatches, record formats, worksheets, textures. |
| Record format | A user-defined set of fields attached to objects. |
| Worksheet | A spreadsheet that reads object parameters and records by criteria. |
| Report | A preformatted worksheet, such as a plant schedule. |
| Data tag | A label linked to an object. Shows chosen fields. Uses a tag style. |
| Data visualization | A rule that recolors objects by criteria. |
| Graphic legend | A sheet-layer table of object views or attributes. |
| Site model | A terrain surface built from source data and modifiers. Has existing and proposed surfaces. |
| Site modifier | An object that changes the proposed surface: pad, grade limits, grade object. |
| Grade limits | A closed boundary that limits where modifiers act. |
| Plant style | A reusable plant definition: name, ID, spread, height, symbols, database fields. |
| Plant tag | A label showing plant fields at up to three levels. |
| Landscape area | A polygon planted with mixed species by rate or percentage. |
| Hardscape | A closed area or path with a style (surface and components). |
| Existing tree | A surveyed tree with canopy spread, kept or removed. |
| Georeferencing | A coordinate system plus origin offset, stored per design layer. |
| Phase | A status (existing, new, demolished) on objects. Filters views and exports. |
| Command | One serializable edit. Executed through the editor. Always undoable. |
| Patch | The before/after record of a command. Powers undo and redo. |
| Agent API | JSON-RPC 2.0 interface that runs commands and reads state. |

## 4. Feature tables

### 4.1 Document model

| ID | Feature | Pri | Status |
|---|---|---|---|
| DOC-01 | Document with layers, classes, objects, active layer/class. | P0 | done |
| DOC-02 | Object with shape, layer, class, attribute overrides. | P0 | done |
| DOC-03 | Design layers: add, modify, delete, lock, hide. | P0 | done |
| DOC-04 | Classes: add, modify, visibility. | P0 | done |
| DOC-05 | Attribute resolution (object overrides class defaults). | P0 | done |
| DOC-06 | Visibility rules (layer and class). | P0 | done |
| DOC-07 | Command pattern with Batch and atomic rollback. | P0 | done |
| DOC-08 | Undo/redo through patches. | P0 | done |
| DOC-09 | Locked layer rejects edits. | P0 | done |
| DOC-10 | Native file format (JSON based, versioned). | P0 | planned |
| DOC-11 | Layer elevation, stories, 3D height. | P1 | planned |
| DOC-12 | Class and layer visibility modes: visible, grey, invisible. | P1 | planned |
| DOC-13 | Saved views: layer and class visibility, active layer and class, zoom and pan, 3D view. Palette list, `saved_view.*`. | P1 | done |
| DOC-14 | Phases (existing, new, demolished). | P1 | planned |
| DOC-15 | Symbols and symbol definitions: a resource with 2D components and optional 3D components (hybrid), an insertion point, and instances (`Shape::Symbol`) with position, rotation, symmetric or asymmetric scale and a class. Editing the definition changes all instances at once. Convert to group, Edit 2D components in place (`edit_symbol`, `exit_symbol_edit`), nested symbols (loops are refused). Plug-in object symbols (red and blue) are out of scope. | P1 | done |
| DOC-16 | Resource libraries: import resources of any type from another `.cdk.json` file by type and name, with what they need (a tile brings its symbol, which brings its hatch), classes of symbol components by name, rename on a name clash (or reuse, or replace), one undo step. Classes and design layers can be imported by name. A per-user library folder (`CADEKTOR_LIBRARY_DIR`, default `~/.config/cadektor/library`), favorites, export of a library file. File access follows the path policy. | P1 | done |
| DOC-17 | Project sharing, file merge. | P2 | planned |
| DOC-18 | Document health check. | P2 | planned |

### 4.2 2D drafting

| ID | Feature | Pri | Status |
|---|---|---|---|
| DRF-01 | Line, rectangle, circle, arc, polyline, text shapes. | P0 | done |
| DRF-02 | Move objects. | P0 | done |
| DRF-03 | Shape validation (reject bad geometry). | P0 | done |
| DRF-04 | Bounds and area calculation. | P0 | done |
| DRF-05 | Selection tool: click, rectangle, lasso, shift-add. | P0 | planned |
| DRF-06 | Tools for the shapes above with live preview. | P0 | planned |
| DRF-07 | Rotate, scale, mirror, offset, duplicate array. | P0 | planned |
| DRF-08 | Fillet, chamfer, trim, extend, split. | P1 | planned |
| DRF-09 | Polyline editing (vertices, smooth, Bezier, spline). | P1 | done |
| DRF-15 | Curved polylines. A vertex is a corner, a fillet arc with a radius, a point on an arc, a quadratic Bezier control point or a cubic point that the curve passes through (centripetal Catmull-Rom). Landscape area and hardscape boundaries use the same vertices. A plain `[x, y]` in a file is a corner. | P1 | done |
| DRF-16 | Exact curve math: arc radius is clamped to fit, area is exact (rounded square = s² − (4 − π)r²), bounds and hit tests use a chord tolerance. SVG output uses true `A`, `Q` and `C` path commands. | P1 | done |
| DRF-17 | `set_vertex` command: change the kind, radius or position of one vertex, undoable. | P1 | done |
| DRF-18 | Vertex mode buttons in the tool bar for Polyline, Landscape Area and Hardscape: Corner, Bézier, Cubic, Tangent arc, Point on arc, Arc. The Arc radius is a default that stays until changed. | P1 | done |
| DRF-19 | Object Info vertex list: index, X, Y, kind and radius, all editable. Selected paths show vertex markers. | P1 | done |
| DRF-10 | Fill, pen, line type, hatch attributes. Attributes `fill_pattern` (a hatch or tile) and `line_type` on objects and classes. Hatches (line families with angle, spacing, offset, dash, color and weight, in paper or model units), tiles (a symbol repeated on a grid with row shift and angle) and line types (dashes, gaps, symbols along the line) are clipped to the shape (holes included) in the canvas, SVG, PDF and PNG sheet output, and written to DXF as plain LINE entities. Textures exist as a resource type only. | P1 | done |
| DRF-11 | Dimensions (linear, angular, radial, area). | P1 | planned |
| DRF-12 | Groups: group and ungroup (one level), nested groups, enter and exit a group (edit context), move, rotate and copy a group as a unit, delete with members, all undoable. A click selects the top group, and inside a group it selects the members. Select-similar is planned. Rotating hardscapes, landscape areas and site models is not built yet. | P1 | done |
| DRF-20 | Resource Manager for all resource types: thumbnails (SVG and egui paths from one list), search, type filter, duplicate, rename, delete, delete unused (frees what unused resources used), import from a file. API: `resource.*`, `library.*`. | P1 | done |
| DRF-21 | A plant style can use a symbol as its plan graphic (`plan_symbol`), scaled to the canopy. | P1 | done |
| DRF-13 | Compose, clip, add/subtract surfaces. | P1 | planned |
| DRF-14 | Pan, zoom, zoom to fit, view history. | P0 | planned |

### 4.3 Snapping and data bar

| ID | Feature | Pri | Status |
|---|---|---|---|
| SNP-01 | Snap to object points: end, mid, centre. | P0 | planned |
| SNP-02 | Grid snap. | P0 | planned |
| SNP-03 | Snapping set (toggle snap categories). | P0 | planned |
| SNP-04 | Smart cursor cues (labels at the cursor). | P0 | planned |
| SNP-05 | Data bar: Tab to enter length, angle, coordinates. | P0 | planned |
| SNP-06 | Tab and Shift+Tab move between fields. Enter commits. | P0 | planned |
| SNP-07 | Angle and distance snap. | P1 | planned |
| SNP-08 | Smart points and smart edges (alignment lines). | P1 | planned |
| SNP-09 | Snap loupe. | P2 | planned |
| SNP-10 | Back-quote key suspends snapping. | P1 | planned |
| SNP-11 | Snap across layers and grey classes (option). | P1 | planned |

### 4.4 UI palettes

| ID | Feature | Pri | Status |
|---|---|---|---|
| UI-01 | App window: menu bar, view bar, tool bar, canvas, status bar. | P0 | planned |
| UI-02 | Tool palettes grouped as tool sets. | P0 | planned |
| UI-03 | Object Info palette (Shape, Data, Render tabs). | P0 | planned |
| UI-04 | Navigation palette (classes, layers, sheets, viewports, views). | P0 | planned |
| UI-05 | Resource Manager palette (plant styles; all other resource types in the section "Resources (all types)", see DRF-20). | P1 | done |
| UI-06 | Quick search for commands. | P1 | planned |
| UI-07 | Dockable, saveable layouts. | P1 | planned |
| UI-08 | Workspaces: editable menus, tool sets, shortcuts. | P2 | planned |
| UI-09 | Keyboard shortcuts compatible with common CAD habits. | P0 | planned |
| UI-10 | Light and dark themes. | P1 | planned |
| UI-11 | Tool modes in tool bar. Keys jump between mode groups. Done for the vertex modes of the path tools (group 1, key U). | P1 | done |
| UI-12 | Undo history list. | P2 | planned |

### 4.5 Site model and grading

| ID | Feature | Pri | Status |
|---|---|---|---|
| SIT-01 | 3D point source data: survey points, 3D loci, 3D polygons. | P0 | partial: 3D loci and 3D polylines move into a site model (survey point import comes with the file format work) |
| SIT-02 | Triangulated existing surface. | P0 | done |
| SIT-03 | Proposed surface from modifiers. | P0 | done |
| SIT-04 | Pad modifier. | P0 | done (flat or sloped by elevation at end; overlapping pads nest, the later one wins; exact vertical edge without grade limits, no gap) |
| SIT-05 | Grade limits. The ground outside does not change (overlay of the existing triangles). | P0 | done |
| SIT-11 | Site Modifiers tool with modes, crop boundary, Update Site Model (auto update off shows out of date), site.drape. | P1 | done |
| SIT-06 | Cut and fill volumes (proposed minus existing). | P0 | done (exact on the overlay of both surfaces) |
| SIT-07 | Contours for existing and proposed surfaces. | P0 | done (major and minor, smooth option, labels) |
| SIT-08 | Elevation lookup at a point. | P0 | done (`site.elevation`, draping API in `cadektor-site`) |
| SIT-09 | Display choice: existing, proposed, both (2D and 3D). | P1 | done |
| SIT-10 | Pad with retaining edge. | P1 | planned |
| SIT-18 | Grade object (tangent, arc, cubic, polyline) with end tags. | P1 | planned |
| SIT-12 | Aligned modifiers (to hardscape, wall, polygon). Hardscapes are done (HRD-10). | P1 | planned |
| SIT-13 | Stake / spot elevation object. | P1 | planned |
| SIT-14 | Source data checks: find errors, simplify. | P1 | planned |
| SIT-15 | Slope and aspect analysis display. | P2 | partial: slope and aspect at a point, no legend display |
| SIT-16 | Point cloud as source (LAS/E57). | P2 | planned |
| SIT-17 | Volume fill and grading impact report. | P2 | planned |

### 4.6 Plants

| ID | Feature | Pri | Status |
|---|---|---|---|
| PLT-01 | Plant shape: position, species, spread, height. | P0 | done |
| PLT-02 | Plant report (counts by species) via `report.plants`. Rows of plants with a style also give style, ID code and prices. | P0 | done |
| PLT-03 | Plant style resource (name, ID, spread, height, category, 2D graphic, 3D form, colors, price). Plants refer to a style and can override fields. Undoable add, edit and delete. A style in use cannot be deleted unless the plants are reassigned. | P0 | done |
| PLT-04 | Plant database fields: botanical and common name, size, container, root type and custom fields. | P1 | done |
| PLT-05 | 2D plant symbol rendering: generated outlines (circle, cloud, star, spokes, rosette, clover) with style colors. | P0 | done |
| PLT-06 | Plant tags: a tag object with a leader line shows the ID code and the count of a style on its layer. The position is editable. | P1 | done |
| PLT-07 | Resource Manager palette for plant styles: list, create, edit, duplicate, delete, CSV import and export. | P1 | done |
| PLT-08 | Create plant styles from a list (CSV mapping): columns by header or explicit mapping, units per column, skip or update duplicates, and CSV export. | P1 | done |
| PLT-09 | 3D plant representation as generated geometry: tree, columnar, conical, spreading and shrub mound forms. | P1 | done |
| PLT-10 | Place plants on terrain height. | P1 | planned |
| PLT-11 | Plant schedule: live `report.plant_schedule` with counts and prices, CSV export and a palette in the app. | P0 | done |
| PLT-12 | Import plant data from nursery lists as CSV (see PLT-08). | P2 | done |
| PLT-13 | Plant tool modes: single, along a path (spacing, count or both), rectangular grid, triangular grid, fill an area (spacing, count or density) and poly. One undo step per placement. The placement math is a pure, tested module. | P0 | done |
| PLT-14 | Existing trees: trunk diameter (DBH), canopy spread, height, condition, action (retain, remove, relocate) and a root protection zone of 12 x DBH (configurable per tree). The zone is drawn dashed. Trees to remove are drawn dashed with a cross. `report.existing_trees` lists them. | P0 | done |
| PLT-15 | Plant styles in files: a document `resources` list. Old files load unchanged. Plants without a style keep their own species, spread and height. | P0 | done |
| PLT-21 | Locked style fields: a style lists fields that plants cannot override (by style). Other fields can be set per plant (by instance). | P1 | done |
| PLT-22 | Seeded size and rotation variation on placement. The same seed gives the same plants. | P1 | done |
| PLT-23 | Quantity by area for groundcovers in the plant schedule (price per m2). | P1 | done |
| PLT-24 | Tree protection zone for plants of the tree category, with the rule and cap of existing trees (12 x trunk diameter, at most 15 m). | P1 | done |
| PLT-25 | Fill an area with holes, and spacing equal to the plant spread. | P1 | done |
| PLT-16 | Plant images, maintenance and irrigation fields in plant styles. | P1 | planned |
| PLT-17 | Plant tags with three levels of fields and an ID bubble. | P1 | planned |
| PLT-18 | Plant style spreadsheet view for batch editing. | P1 | planned |
| PLT-19 | 3D plants from symbols and billboards. | P2 | planned |
| PLT-20 | Copy plant data from partner nursery sites. | P2 | planned |

### 4.7 Landscape areas

| ID | Feature | Pri | Status |
|---|---|---|---|
| LND-01 | Landscape area shape: boundary (corners and curved vertices) and name. | P0 | done |
| LND-02 | Area calculation. | P0 | done |
| LND-03 | Area report via `report.areas`. | P0 | done |
| LND-04 | Landscape area style resource: a plant mix (plant styles from the resource store), 2D fill and border, data fields, and a default for "Show plants". Undoable add, edit and delete. A plant style in a mix cannot be deleted unless it is reassigned. | P0 | done |
| LND-05 | Mix by spacing or density per m2. Without either, the canopy of the plant gives the spacing. | P0 | done |
| LND-06 | Mix by percentage of the area with minimum counts. | P1 | done |
| LND-07 | Representative plant scatter in 2D: deterministic symbols from the mix, outside the holes, tree protection zones included. | P1 | done |
| LND-08 | Quantities per plant style from the net area (`report.landscape_areas`). The plant list (`report.plant_list`) adds them to the single plants, one row per style. Groundcovers counted by area keep their price per m2. | P0 | done |
| LND-09 | Hedgerow mode (staggered rows). | P1 | planned |
| LND-10 | Hydrozone and water budget reports. Data fields of the style are in the report. | P2 | planned |
| LND-11 | Holes: parts of an area that stay empty (`landscape_area.set` with holes or exclude). They move with the area. Overlapping holes count once, and holes must stay inside the boundary. | P1 | done |
| LND-12 | Draped landscape area: a 3D face on the site model, and the draped surface area in the report. | P1 | done |
| LND-13 | Edit modes in Object Info (style, show plants, drape, holes) and a style picker in the Landscape Area tool. | P1 | done |

### 4.8 Hardscapes

| ID | Feature | Pri | Status |
|---|---|---|---|
| HRD-01 | Hardscape shape: boundary (corners and curved vertices) and material. | P0 | done |
| HRD-02 | Hardscape style resource: surface material and thickness, build-up components (layers with material and thickness), border (width, material, both sides or one) and a joint pattern. Undoable add, edit and delete. | P1 | done |
| HRD-03 | Quantities (`report.hardscape`): plan area, surface area (slope or draped), perimeter, and the volume of every layer. Materials are added up. | P1 | done |
| HRD-04 | Path alignment and offset: the drawn line is the center or the left or right edge of the path, with a sideways offset. Slab edge offsets are not done. | P1 | done |
| HRD-05 | Path hardscape: a center line (curves allowed, open or closed) with a width and miter, round or bevel joins. The outline is a union of the legs and the joins, so tight curves and short legs make no loops. The area of a straight path is exactly length times width. | P1 | done |
| HRD-06 | Slope: none, by edge elevations (a plane through the corner heights; start and end heights of a path), a fall in percent, a fall by angle, or a rise over a run. Edge heights follow an outline edit. | P2 | done |
| HRD-08 | Joint patterns in 2D: running bond, stack, herringbone and basket weave, with unit size, joint width and angle. | P1 | done |
| HRD-09 | Drape on site model in 3D (slab, components and walls follow the ground, a few mm above it) and in the area report. Beyond the edge of the model the height of the nearest edge counts, and the report says `partially_draped`. | P1 | done |
| HRD-10 | Site modifier option: the hardscape acts as a pad at its formation level (top minus build-up), sloped as the hardscape. | P2 | done |
| HRD-11 | Edit modes in Object Info (style, slab or path, width, join, slope, elevation, drape, site modifier) and style, path and width options in the Hardscape tool. | P1 | done |
| HRD-07 | Landscape walls. | P2 | planned |

### 4.9 Existing trees

| ID | Feature | Pri | Status |
|---|---|---|---|
| TRE-01 | Existing tree object: position, species, trunk, canopy spread. | P1 | planned |
| TRE-02 | Status: keep, remove, protect. | P1 | planned |
| TRE-03 | Canopy spread in eight directions. | P2 | planned |
| TRE-04 | Tree survey import from CSV. | P1 | planned |
| TRE-05 | Tree report. | P1 | planned |
| TRE-06 | Export trees as Shapefile. | P2 | planned |

### 4.10 GIS and georeferencing

| ID | Feature | Pri | Status |
|---|---|---|---|
| GIS-01 | Georeference: CRS plus origin offset, rotation and user origin. One per document (per design layer is planned). | P0 | done |
| GIS-02 | CRS from EPSG code. | P0 | done |
| GIS-03 | CRS from WKT text and `.prj` file. | P0 | done |
| GIS-04 | Coordinate transform (via a PROJ-compatible library). Uses `proj4rs`. | P0 | done |
| GIS-05 | Shapefile import (points, lines, polygons with holes, Z values, dbf attributes). Export is planned. | P0 | done |
| GIS-06 | GeoJSON import and export. | P0 | done |
| GIS-07 | Survey point tool (set known point). | P1 | planned |
| GIS-08 | Georeferenced images (world file, GeoTIFF). | P1 | planned |
| GIS-09 | Web map tiles and WMS as backdrop. | P2 | planned |
| GIS-10 | Northing/Easting grid display. | P1 | planned |
| GIS-11 | Georeference carried through IFC and DXF export. IFC: `IfcMapConversion`, `IfcProjectedCRS` and site latitude/longitude are done. DXF: planned. | P1 | partly done |

### 4.11 Irrigation

Crate `cadektor-irrigation` (pure, wasm-compatible) holds the hydraulic model. The drawing objects are one `Shape::Irrigation` variant in the core (`irrigation.rs`). Units: mm, degrees, kPa, l/min.

| ID | Feature | Pri | Status |
|---|---|---|---|
| IRR-01 | Plant irrigation record fields (water need, hydrozone). | P1 | planned |
| IRR-02 | Outlet objects (spray head, rotor, drip emitter, bubbler): a point with model, spray arc, direction and radius. They draw a translucent coverage fan in 2D, in the color of the zone. Undoable through `add_object`, `modify_object`, move and rotate. | P1 | done |
| IRR-03 | Irrigation lines (mainline, lateral, drip line) with pipe size and material. Points of connection (static pressure, available flow), control valves (zone, flow factor kv, controller and station) and controllers (stations). | P1 | done |
| IRR-04 | Zone assignment and coverage: an outlet belongs to the zone of the nearest valve upstream. "Calculate" stores the zone in the outlets (one undo step). Colors per zone. | P1 | done |
| IRR-05 | Reports: zone schedule (heads per model, flow, area, precipitation rate, run time for a target depth), pipe schedule (length per type, material and size), pressure analysis per zone and a materials list, as JSON and CSV. Worksheet templates `irrigation_heads` and `irrigation_pipes`, and worksheet properties (Model, Zone, Arc, LineType, Size, Material, Length, ...). | P1 | done |
| IRR-06 | Water budget report. | P2 | planned |
| IRR-07 | Irrigation catalog as document resources (head models with radius, arc range, flow and pressure; pipe sizes with inner diameter; pipe materials with the Hazen-Williams coefficient) over a built-in catalog. CSV import and export with the plant catalog reader. | P1 | done |
| IRR-08 | Hydraulics: Hazen-Williams pressure loss with C by material, valve loss from kv, velocity check (warning above 1.5 m/s), precipitation rate from the flow and area. Documented formulas and units, unit tests against hand calculations. | P1 | done |
| IRR-09 | Network from geometry: points within a tolerance (default 50 mm) are one node. Tree solving POC, mainline, valves, laterals, outlets for one zone at a time. Pressure at every outlet, critical outlet and path, warnings (low pressure, velocity, supply flow, unconnected outlets, loops, catalog gaps, spray arc range, controller stations). | P1 | done |
| IRR-10 | Layout helpers (pure functions with property tests): fill an area with heads in a square or triangular pattern at head-to-head spacing, heads along an edge, parallel drip lines in an area. | P1 | done |
| IRR-11 | Agent API `irrigation.*` with `describe` examples: `outlet.add`, `line.add`, `poc.add`, `valve.add`, `controller.add`, `set`, `catalog.get|set|import_csv|export_csv`, `calculate`, `layout.fill|edge|drip`, `report.zones|pipes|pressure|materials`. | P1 | done |
| IRR-12 | Irrigation tool in the app (`Alt+Shift+I`) with modes Outlet, Line, POC, Valve and Controller (`U` cycles the modes), snapping that joins line ends, fields and catalog lists in Object Info, and a Calculate command that shows the result and the warnings in Object Info. | P1 | done |
| IRR-13 | Elevation and slope in the pressure calculation, flow that changes with pressure, loops (looped mains), master valves with several zones at once. | P2 | planned |
| IRR-14 | Irrigation objects in IFC and DXF export. | P2 | planned |

### 4.12 Reports, worksheets, data tags

| ID | Feature | Pri | Status |
|---|---|---|---|
| RPT-01 | Plant and area reports through the API. | P0 | done |
| RPT-02 | Record formats: typed fields (text, integer, number, boolean, dimension, area, choice list) with defaults and units, as resources. Attach, detach and edit records on objects (undoable, validated). Record format editor and Data tab of Object Info. | P0 | done |
| RPT-03 | Worksheet resource (`cadektor-sheet`): cells, formulas (+ - * / ^ &, comparisons, references A1 and B2:C5, SUM AVERAGE MIN MAX COUNT IF ROUND CONCAT and more), formats (decimals, unit), live recalculation cached by revision, cycles give #CYCLE!. Worksheet window with grid and formula bar. | P1 | done |
| RPT-04 | Preformatted reports: `report.plant_list`, `report.landscape_areas` and `report.hardscape` are done, with CSV. Printable layouts are not. | P0 | planned |
| RPT-05 | Export worksheet to CSV. | P0 | done |
| RPT-06 | Data tag styles and tag placement. | P1 | planned |
| RPT-07 | Data visualizations (color by criteria). | P1 | planned |
| RPT-08 | Graphic legends. | P2 | planned |
| RPT-09 | Worksheet database rows linked to live objects: criteria (kind, layer, class, record, field comparisons, style), column expressions (Area, Length, Perimeter, Count, Layer, Class, Style, plant and record fields), sort, group, subtotals and totals. Database setup in the worksheet window. | P1 | done |
| RPT-10 | Export worksheet to XLSX (values and number formats, own small writer, no extra crate). | P2 | done |
| RPT-11 | Worksheet table object: place a worksheet on a design layer or a sheet layer (`worksheet.place`, `AddWorksheetTable`). It draws the calculated cells, follows the document, prints in PDF, sheet SVG and PNG, and obeys the sheet layer rules. Criteria builder in the worksheet window. | P1 | done |

### 4.13 Sheets, viewports, annotation

| ID | Feature | Pri | Status |
|---|---|---|---|
| SHT-01 | SVG render of a layer set. | P0 | done |
| SHT-02 | Sheet layer with paper size, orientation, sheet number and title (paper space, mm, 1:1). Separate section in the Navigation palette. `sheet.*`. | P1 | done |
| SHT-03 | Viewport object: design layers at a scale, plan (vector) or 3D standard view (raster), rectangular or polygon crop, live update cached by revision. `viewport.*`. | P1 | done |
| SHT-04 | Viewport layer and class visibility overrides and class graphic overrides (pen, fill, line weight). Order: object attributes, viewport class override (by-class attributes only), class attributes, defaults. A viewport keeps its own layer and class visibility. Layer and class graphic "grey" mode is missing. | P1 | done |
| SHT-05 | Annotation space in a viewport (paper units, relative to the frame): line, text, dimension, any 2D shape. `viewport.annotate`. Annotation tools in the app are still missing. | P1 | partial |
| SHT-06 | PDF export of sheets (`pdf-writer`): vector lines with weights, fills, arcs as curves, Helvetica text, 3D viewports as images. Also SVG and PNG per sheet. File > Export PDF, `export.file` with format `pdf`. | P1 | done |
| SHT-07 | Batch print / publish. | P2 | planned |
| SHT-08 | Section and elevation viewports. | P2 | planned |
| SHT-09 | Design layer viewports. | P2 | planned |
| SHT-10 | Title block border: parametric object with border, grid reference and a title block with fields from project and sheet data. Standard layout or JSON layout. `title_block.*`, `project.*`. | P1 | done |
| SHT-11 | North arrow and scale bar. | P1 | planned |

### 4.14 3D and rendering

| ID | Feature | Pri | Status |
|---|---|---|---|
| 3D-01 | wgpu 3D view: orbit, pan, zoom to cursor, zoom to fit, click to select. | P1 | done |
| 3D-02 | Standard views (top, bottom, front, back, left, right, four isometrics), parallel and perspective. | P1 | done |
| 3D-03 | Extrude, sweep, 3D polygon. | P1 | planned |
| 3D-04 | Render modes: wireframe and shaded. | P1 | done |
| 3D-10 | Render mode: hidden line. | P2 | planned |
| 3D-05 | Terrain mesh with texture. | P1 | partial: terrain mesh and contours in 3D, no texture |
| 3D-06 | Textures and materials. | P2 | planned |
| 3D-07 | Lights, sun position by georeference and date. | P2 | planned |
| 3D-08 | Raster render styles. | P2 | planned |
| 3D-09 | Export OBJ and STL. | P2 | planned |

### 4.15 Architecture (P1/P2)

| ID | Feature | Pri | Status |
|---|---|---|---|
| ARC-01 | Wall object with style, height, components. | P1 | planned |
| ARC-02 | Door and window in walls. | P1 | planned |
| ARC-03 | Slab and roof. | P1 | planned |
| ARC-04 | Stories linked to layers. | P1 | planned |
| ARC-05 | Space object with area. | P2 | planned |
| ARC-06 | Stairs. | P2 | planned |
| ARC-07 | Columns and beams. | P2 | planned |
| ARC-08 | Section viewports from the model. | P2 | planned |
| ARC-09 | Massing model. | P2 | planned |

### 4.16 Interop

| ID | Feature | Pri | Status |
|---|---|---|---|
| IOP-01 | DXF import and export (2D, layers, blocks). ASCII DXF. Bulges stay exact arcs. | P0 | done |
| IOP-02 | IFC4 export (IFC4 ADD2 TC1) with property sets, quantities, materials, styles and georeference. Crate `cadektor-ifc`, `export.file` with format `ifc`. Terrain, hardscape (`IfcSlab`), landscape areas and plants (`IfcGeographicElement`). Validated with IfcOpenShell. IFC4 has no VEGETATION type, so vegetation is USERDEFINED with ObjectType VEGETATION. | P0 | done |
| IOP-03 | IFC4 import (geometry, property sets, spatial tree). | P0 | planned |
| IOP-04 | IFC 2x3 import and export. | P1 | planned |
| IOP-05 | IFC4x3 (site, terrain, georeference). | P1 | planned |
| IOP-06 | Shapefile and GeoJSON (see GIS). | P0 | done |
| IOP-07 | Data mapping rules to IFC fields: a Data Manager mapping table (resource) from object parameters to record fields and IFC property sets, with an evaluation function and API. The IFC writer itself is IOP-02. | P1 | done |
| IOP-15 | IFC export options: classes become presentation layers, 2D annotations on or off, design layers as building storeys on or off, a named data map. GlobalIds stay stable (project GUID in the document). | P1 | done |
| IOP-08 | PDF export. | P1 | done |
| IOP-09 | DXF 3D entities. | P1 | planned |
| IOP-10 | DWG import and export. | P2 | planned |
| IOP-11 | OBJ, STL, STEP. | P2 | planned |
| IOP-12 | Point cloud import. | P2 | planned |
| IOP-13 | Native `.vwx`. | Deferred | planned |
| IOP-14 | Import report that lists dropped or approximated items. | P1 | done |

### 4.17 Agent API

| ID | Feature | Pri | Status |
|---|---|---|---|
| API-01 | JSON-RPC 2.0 dispatcher in core. | P0 | done |
| API-02 | `describe` method lists all methods. | P0 | done |
| API-03 | `execute`, `undo`, `redo`. | P0 | done |
| API-04 | `document.summary`, `document.get`, `document.load`. | P0 | done |
| API-05 | `objects.list`, `objects.get`. | P0 | done |
| API-06 | `selection.get`, `selection.set`. | P0 | done |
| API-07 | `render.svg`. | P0 | done |
| API-08 | `report.plants`, `report.areas`. | P0 | done |
| API-40 | Plant methods: `plant_style.*` (list, get, create, update, duplicate, delete, import_csv, export_csv, sample_csv), `plants.place`, `plants.set_style`, `report.plant_schedule`, `report.existing_trees`. Existing trees and plant tags are shapes that `execute` adds. | P0 | done |
| API-41 | Drawing documentation methods: `sheet.*`, `project.*`, `viewport.*`, `title_block.*`, `saved_view.*`, `export.file` with the formats `pdf`, `sheet_svg`, `sheet_png`. Their examples are in `describe`. | P1 | done |
| API-09 | Serializable Command enum (add/delete/move/modify, set_vertex, set_data, layers, classes, batch). | P0 | done |
| API-10 | `cadektor serve` (stdio). | P0 | done |
| API-11 | TCP listener in the live app (`--agent-port` or `CADEKTOR_AGENT_PORT`). A random token is required as the first message. | P0 | done |
| API-12 | `cadektor mcp` server for Claude Code. | P0 | done |
| API-13 | `render.png`: a CPU render of the 3D model, base64 PNG, also in `cadektor serve`, over `--connect` and in MCP (as an image). | P1 | done |
| API-14 | Screenshot of the live viewport. | P1 | planned |
| API-15 | Command schema (JSON Schema) in `describe`. | P1 | planned |
| API-16 | Query by criteria (class, layer, record field). | P1 | planned |
| API-17 | Event stream: document changed, selection changed. | P2 | planned |
| API-18 | Import/export methods (DXF, IFC, GIS). | P1 | planned |
| API-19 | `view.list` and `view.set`: choose the view of the live app. | P1 | done |
| API-41 | Data and report methods: `record_format.*`, `record.attach`, `record.detach`, `record.set`, `record.get`, `data_manager.*` (list, get, create, update, delete, check, evaluate, apply, properties), `worksheet.*` (list, templates, create, get, rename, delete, set_cell, set_cells, set_database, remove_database, evaluate, export, place, fit). Examples in `describe`. | P0 | done |
| API-30 | `group.*` (create, ungroup, enter, exit, context, members, move, rotate, copy) and `selection.click`. | P1 | done |
| API-31 | `symbol.*` (list, get, create, from_objects, place, set, convert_to_group, edit, edit_end). | P1 | done |
| API-32 | `resource.*` (list, get, create, update, rename, duplicate, delete, purge_unused, thumbnail, list_file, import_from_file, export_to_file), `pattern.segments`, `library.*` (list, favorites). `describe.resources` has examples. | P1 | done |

### 4.18 Scripting

| ID | Feature | Pri | Status |
|---|---|---|---|
| SCR-01 | Python bindings to the command API. | P1 | planned |
| SCR-02 | Script console palette. | P1 | planned |
| SCR-03 | Script files as resources. | P2 | planned |
| SCR-04 | Parametric plug-in objects defined by script. | P2 | planned |
| SCR-05 | Export a drawing as a script (replay commands). | P2 | planned |
| SCR-06 | Visual node scripting. | P2 | planned |
| SCR-07 | Python import path for shared Vectorworks-style function names. | P2 | planned |

## 5. Non-functional requirements

- Performance: draw 100,000 simple objects at 60 fps pan and zoom on a mid-range laptop.
- Performance: a command on a small selection completes in under 10 ms.
- Determinism: the same command list on the same document always gives the same result.
- Safety: a failed command or batch leaves the document unchanged.
- Portability: core builds for native and `wasm32-unknown-unknown`. No threads or file access in core.
- Platforms: macOS, Linux, Windows desktop. Web build is best effort.
- Accuracy: use 64-bit floats. Document units and tolerances in one place.
- File safety: write files atomically. Keep a backup on save.
- Accessibility: keyboard access for all tools. Scalable UI text.
- Localization: all UI text goes through one string table. English first.
- Quality: core line coverage of 85% or more. Property tests for geometry and undo.
- Docs: every API method appears in `describe`.
- Privacy: no telemetry in MVP.

## 6. Release plan

MVP (P0)
- Document model, 2D drafting, snapping, data bar, basic palettes.
- Plants, landscape areas, hardscapes as shapes with styles.
- Basic site model: points, existing surface, pad, grade limits, contours, cut/fill.
- Shapefile, GeoJSON, georeferencing with EPSG and WKT.
- DXF 2D and IFC4 basics.
- Reports as CSV.
- Agent API over stdio, TCP, and MCP.
- CI on three platforms.

v1 (P1)
- Sheet layers, viewports, PDF export.
- Plant Style Manager, plant tags, data tags, worksheets, data visualizations.
- Grade object, stakes, existing trees.
- IFC 2x3 and 4x3, DXF 3D, phases, saved views.
- 3D view, terrain mesh.
- PNG render, viewport screenshot, Python scripting.
- First architecture objects: walls, doors, windows, stories.

Later (P2)
- Irrigation design, water budget.
- Graphic legends, section viewports, project sharing.
- DWG, point clouds, STEP.
- Materials, lights, sun study.
- Workspaces, visual scripting, parametric plug-in objects.
- If legal review allows, native `.vwx`.

## 7. Legal notes

- Clean-room rule: build from public behavior and open standards only. Do not read Vectorworks code, SDK headers under restrictive terms, or leaked material.
- Do not copy help text, tooltips, icons, tool art, or screenshots.
- Do not use Vectorworks, Landmark, Spotlight, Renderworks, Architect, or other marks in the product name, UI, or marketing. Describe compatibility in plain words only where law allows.
- Use generic terms for product names ("landscape tools", "render styles").
- Native `.vwx` read/write waits for legal review. Do not reverse-engineer files until that review ends.
- IFC, DXF, Shapefile, GeoJSON, and EPSG data come from open or documented specs. Check the license of each library and dataset before use.
- Dependencies must be compatible with Apache-2.0 OR MIT. Keep a third-party license list.
- Plant data and symbol libraries must have a clear, open license.
- Research notes cite public pages only. Mark unverified items as such.
