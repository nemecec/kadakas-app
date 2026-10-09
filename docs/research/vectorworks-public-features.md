# Vectorworks public feature research (for CAD clone design)

Scope: Vectorworks 2026 is public (service releases up to Update 6, July 2026). Sources are public web pages only; nothing was downloaded. Text is paraphrased. Items marked (unverified) were not confirmed on a fetched page.
Note: the vectorworks.net/en-US/2026 "What's New" page appears to also describe upcoming/next-version items (image names say 2027). Treat those as roadmap, not shipped 2026.
Several app-help pages returned 403 to direct fetch (e.g. Site_models.htm); details came from search excerpts.

## 1. Core workflow and concepts (shared by all products)

Sources: https://app-help.vectorworks.net/2026/eng/VW2026_Guide/LandingPage/New_features.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Structure/The_Navigation_palette.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Objects_edit1/The_Object_Info_palette.htm

- Design layers hold the model (2D+3D, one shared drawing space). Sheet layers hold the printable sheets. Viewports on sheet layers show a design-layer view at a scale, with their own layer/class visibility and overrides, plus annotations that live on the sheet.
- Design layer viewports also exist: an object on a design layer that shows another view. It shares the layer's scale/render mode and cannot take annotations.
- Classes are the main attribute/visibility grouping (a second axis next to layers). Visibility is set per class and per layer (visible / grey / invisible), and overridden per viewport.
- Navigation palette (Cmd/Ctrl+Shift+N): one non-modal palette with tabs for Classes, Design Layers, Sheet Layers, Viewports, Saved Views, References. Has filter and name search. It is the modeless version of the Organization dialog. 2026 added a Tags column (tag assignment per class/layer).
- Saved views store a combination of layer/class visibility, view, and (2026) can sync with tags/legends/phases.
- Stories/levels: stories are linked to layers with an elevation and heights; used for BIM-style models. (Details not fetched; unverified.)
- Resource Manager: browser for reusable resources: symbols, hatches, record formats, worksheets, textures, scripts, and in 2026 plant, landscape area and hardscape styles (new filters). Thumbnails now follow the drawing's class visibility.
- Object Info palette (OIP): three tabs. Shape (geometry, class, layer, location, object parameters), Data (record formats, IFC data), Render (textures, mesh smoothing). Contents depend on the selection. 2026 adds a per-viewport raster DPI setting, and is the hub for phase data.
- Record formats: user-defined field sets attached to objects. Used by worksheets, data tags, reports and IFC mapping.
- Data Manager (2026): criteria-based mapping from object parameters/record fields to IFC fields; supports IFC spatial hierarchy, zones, systems, groups; settings can be saved and translated between IFC versions.
- Data tags: Data Tag tool places tag styles linked to objects (or label-only). Tag styles are created from an object that has data attached. Subpart mode tags parts of objects. 2026: tags on grade endpoints, stake tool, phasing.
- Worksheets and reports: spreadsheet objects reading object params and records via criteria; 2026 modernised window, filter by fill/pen, object-style/data-tag functions. Landscape area schedules are preformatted reports (Tools > Reports).
- Graphic Legends: sheet-layer-only tool; each cell shows an object view or an attribute (line type, material); legend styles in libraries. 2026 notes: legends can be auto-created from data visualizations.
- Data visualizations: colour/attribute overrides driven by object criteria, for viewports/design layers.
- Phasing (2026 Update 4): objects get phase status (existing/new/demolished/etc.); active phase and shown phases in the View bar; phases apply to viewports, worksheets, tags, legends, saved views; carried through IFC/DWG export.
- Project sharing: a file is converted to a shared project (.vwxp); each user has a local working file, commits changes, and objects/layers are checked out. Admin permission for the creator. Locations: network share, cloud folder, or Project Sharing Server. One working file per user; not in VMs. (https://app-help.vectorworks.net/2026/eng/VW2026_Guide/ProjectSharing/Sharing_a_project_file.htm)
- UI layout: Application window = menus, View bar (view/zoom/render/layers/class controls; modes regular, auto-hide, compact; 2026 allows onscreen view control in any corner and Cloud Status group), Tool bar below it (working plane modes left, tool modes in the middle, quick preferences right), Tool palettes grouped as tool sets, Snapping set, Object Info, Navigation, Resource Manager. Workspaces define menus/palettes/tool sets per product and are editable (Workspace Editor, incl. keys). 2026 adds dockable web palettes (roadmap page) and layouts that carry between computers. https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Start/Application_window.htm ; https://www.vectorworks.net/en-US/newsroom/palettes-workspaces-and-ui-in-vectorworks
- 2D/3D: one model with Top/Plan view and 3D views; objects have a 2D (plan) representation and 3D geometry (walls: Top/Plan now derives from 3D geometry in 2026). Viewports render in wireframe to Renderworks/"render styles" (renamed in 2026; Shaded, Showcase styles added). Depth cueing for viewports added.
- Quick Search (Cmd/Ctrl+F style command finder) got a better algorithm in 2026; Select Coincident Objects shows stacking order.

Changes in 2026 vs older: Renderworks styles renamed render styles; Laubwerk plants renamed Maxon Plant; device database removed in Spotlight/ConnectCAD in favour of symbol-based devices; walls modernised; Navigation palette tags column; Close All/Save All; file health checker; V key opens the Visibility tool; preference to show tool help on Status bar vs Tool bar.

## 2. Landmark (landscape) features

Sources: https://www.vectorworks.net/en-US/newsroom/landmark-2026-features ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/SiteModel1/Workflow__Site_modeling.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/SiteModel1/Concept_Site_model_source_data.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/SiteModel1/Concept_Site_model_modification.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Plants1/Workflow_Planting_design.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Plants2/Creating_landscape_areas.htm

### Site model
- Source data: survey points (tabulated text file), 3D loci, 3D polygons/polylines, stake objects, or point clouds. Recommended prep: Validate 3D Data, Simplify 3D Polygons, remove non-ground points (roof, tree loci). Do not move the site model vertically; fix data and regenerate.
- Existing vs proposed: one site model object, two surfaces. Existing = baseline for cut/fill. Proposed = existing plus modifiers. 2D and 3D display each selectable (existing/proposed/both) in the OIP.
- Site modifiers (Site Modifiers tool, modes): pad, pad with retaining edge, grade limits, aligned modifiers (match roads, hardscapes, massing, 3D polygons, landscape areas, walls), plus others like grade objects, texture beds, etc. (list partly unverified).
- Grade limits: closed boundary around modified area; no elevation of their own, snap to existing surface; without them modifiers have vertical edges. Also protect areas from change.
- Generation: nested modifier hierarchy applied in order, surfaces stitched. Cut/fill volumes = proposed minus existing, shown in modifier params (units selectable; needs grade limits).
- Contours (existing/proposed), 3D mesh, texture, stakes, and site analysis outputs (legends for site model analysis in 2026, e.g. slope/aspect style displays - unverified detail).
- Grade object (2026): draw/reshape as tangent, arc, cubic, Bezier, or polyline following terrain; endpoints carry data tags (elevation, ASL); lockable params; schedules; Create Objects from Shapes. Grading impact reporting and Volume Fill (cut/fill/basins) appear on the roadmap page.
- Stake tool: spot elevations with data tags (2026).

### Plants
- Everything is a plant style (Plant tool, 2026 redesigned). Minimum style fields: unique style name, Plant/Tag ID, spread, height. Rich database fields: botanical/common names, images, 2D symbol, 3D representation (Maxon Plant geometry, image props, symbols), maintenance and irrigation info.
- Plant Style Manager (2026): spreadsheet-like batch editing; Copy External Data from partner nursery sites; Create Plant Styles from List (spreadsheet column mapping). No direct Excel export; use a worksheet.
- Plant tags: 3 levels (top/centre/bottom) of fields, optional quantity or ID bubble, custom tags.
- Plant schedules: plant list worksheets and preformatted reports; 2026 Update 6 fixed quantity maths in landscape areas.
- Landscape areas: polygon regions of mixed species, by distribution rate (spacing) or percentage with minimum counts; plant symbols scattered representatively; schedules (mass planting, hydrozone, water budget). Update 4 (Mar 2026) added a Hedgerow mode (staggered rows, individual plants) to replace the old Hedgerow tool.
- Existing Tree tool (2026): Maxon Plant geometry, props, 3D symbols, 8-direction canopy spread in 2D, tree survey import (units handled), GIS integration, export as shapefile, site model interaction.
- Hardscape: closed area/path objects with styles (surface, components); 2026 offsets measured from drawn line rather than hidden centre. Hardscape styles are resources.
- Massing models: per-floor heights/attributes (2026).
- Irrigation: irrigation tools exist but plant irrigation fields are informational and not linked (per help); hydrozone/water budget reports use them. Landscape walls: not verified in this pass.
- Sustainability Dashboard (2026): live Embodied Carbon, Urban Greening Factor, Biomass Density, Biodiversity Net Gain from model data.
- Update 6 (Jul 2026): 171 Landscape Forms furniture symbols; sturdier plant editing under project sharing.

### Georeferencing / GIS
Sources: https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Georeference/GIS_and_georeferencing.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Georeference/Specifying%20document%20georeferencing.htm
- Georeferencing stored per design layer: coordinate system plus origin offset between internal origin and geographic system. Set via Georeference command, Geolocate tool or Survey Point tool.
- Coordinate system sources: .prj / .proj4 / .xml files, GDAL-supported default list, or WKT text. NTV2 grid transform tools.
- Northing/Easting grid separate from user origin grid. Landmark/Architect get full tools; Spotlight limited.
- Import/export shapefiles (one VW object per feature; CRS from .prj; import into new layer; project to the document CRS). Georeferenced images (world files, GeoTIFF, ECW, JP2).
- Feature services: raster from WMS and ArcGIS servers (Esri-licensed, internet required). WFS upload is not supported per 2025 help. GeoJSON: no public mention found.
- GIS stakes, graticule, great-circle drawing. Geolocation also on DWG/IFC/image import and DWG export (roadmap page for 2026). Geolocation wizard with Google imagery on the newer roadmap.

## 3. Import / export formats

Sources: https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Import/Drag_and_drop_importing.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/DXFDWG/DXF_DWG_and_DWF_file_formats.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/IFC/Concept_IFC_overview.htm

- Drag-and-drop import list: 3DS, DXF/DWG, DWF/DWFx, IFC (.ifc .ifcxml .ifczip), IGES, images, EMF (Win), OBJ, Parasolid .x_t, PDF, PICT, point clouds (.las .laz .pts .e57 .ply .xyz), Revit (.rvt .rfa), Rhino .3dm, SAT, Shapefile .shp, SketchUp .skp, STEP, STL, USD. Also FBX native (roadmap).
- DXF/DWG: import 2.5 to 2026 versions, export R12 to 2026. Export options include phases, record formats and texts as block attributes; shared workgroup settings; batch export (roadmap). Civil 3D DWG import on roadmap.
- IFC: 2x3, IFC4, IFC4x3 supported. Certified export IFC2x3 Coordination View 2.0 and IFC4 Reference View 1.2; import of both. Import: IfcSpace becomes Space object; others become "IFC Entity" plug-in objects keeping their IFC type, with property sets and quantity sets (IFC4); user can exclude stories/elements/psets and map phases/classes. Export: default IFC attachments per object type, Data Manager mapping, IFC Data command, phase-filtered export. Revit/IFC import processed on Vectorworks Cloud.
- Other exchange: MVR/GDTF (entertainment), Morpholio Trace, Maxon Redshift live sync, PDF export, shapefile export.
- Clone relevance: a minimal exchange story is DXF/DWG (2D), IFC4 (with property sets), shapefile/GeoTIFF + .prj for site data, OBJ/STL/STEP for 3D, PDF for sheets.

## 4. Scripting / automation

Sources: https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Scripts/Using_scripts.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Marionette/Visual_scripting_with_Marionette.htm ; https://github.com/Vectorworks/developer-scripting
- Four layers: C++ SDK, Python, VectorScript (Pascal-like), Marionette (visual node graph, nodes are Python; reengineered in 2026). Python and VectorScript share one function API, documented in a public GitHub repo.
- No-code scripting: Export Script / Import Script commands generate Python or VectorScript from a drawing; Custom Selection, Custom Tool/Attribute, Custom Modification, Object Visibility commands generate scripts from dialogs.
- Plug-in objects (parametric objects) are scripted objects with a parameter UI in the OIP.
- AI: AI Visualizer (image-prompted generative render in a palette, mask controls, edit tab; 2026 Updates 2, 3, 5) and an in-app AI assistant on the roadmap page. No public general-purpose API besides scripting.

## 5. Keyboard and interaction conventions

Sources: https://app-help.vectorworks.net/2025/eng/VW2025_Guide/Basic1/Drawing_with_the_Data_bar.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Basic2/Snapping_techniques.htm ; https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Start/Application_window.htm
- Data bar: click to start an object, press Tab to enter the first field (length, angle, coordinates); Tab / Shift+Tab move between fields; Enter commits and returns to the canvas. A floating Data bar near the cursor with Data Bar Options quick preference.
- SmartCursor: snap cues and indicators; snap loupe temporarily zooms when several snaps compete. Smart points (alignment/extension lines; remembers up to 3), smart edges (up to 2), snap to object (corners/endpoints/midpoints/centres; master snap points take priority), snap to distance, angle snapping, grid snapping (no visual cue). Snapping set (palette) toggles categories; double-click a snap tool for SmartCursor Settings; the back-quote key (`) suspends snapping temporarily. Snapping keys are reassignable in the Workspace Editor. Snaps can reach objects on other layers/greyed classes via a layer/class option.
- Tool modes: U, I, O, P, [, ] jump between mode groups on the Tool bar. Single-key tool shortcuts are workspace-defined and editable; V opens the Visibility tool in 2026. Shortcut cheat sheet PDF: https://app-help.vectorworks.net/2026/eng/assets/docs/VWKeyboardShortcuts.pdf (not machine-readable here).
- Selection tool with Modes bar: standard modes for rectangle/lasso/polygon selection and "pointer" mode; modifier-key shortcuts exist (unverified).
- Navigation palette: Cmd/Ctrl+Shift+N.

## Top design takeaways
1. Layers (model) + classes (visibility axis) + sheet viewports with per-viewport overrides is the central mental model.
2. Object Info palette with Shape/Data/Render and parametric objects is the main editing surface.
3. Styles as resources (plant, hardscape, landscape area, data tag, legend) managed in a Resource Manager.
4. Data pipeline: record formats -> data tags, worksheets, legends, data visualizations, IFC mapping.
5. Site model: source data + modifiers + grade limits, existing vs proposed surfaces, cut/fill by difference.
6. Plants as styles with database fields and tags, landscape areas with percentage/density quantities.
7. Georeference per layer with CRS from .prj/WKT/GDAL list; shapefile and WMS imports.
8. Tab-for-data-bar entry and a snap palette with SmartCursor cues.
9. Phasing and saved views as cross-cutting state.
10. Plugin/scripting layered: parametric objects, Python API, visual node graph.
