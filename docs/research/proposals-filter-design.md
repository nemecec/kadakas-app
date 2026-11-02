# Proposals: the object filter (REV-12 design note)

Scope: the first step of REV-12. The row asks for a short design note with the list of every feature that lists objects. This note is that list, plus the design of the central filter. It changes no code.

Method: reading and `grep` over the Rust workspace, `python/`, `docs/` and `scripts/`. No build ran. Line numbers are those of `main` on the day of writing.

## 1. Summary

- Today nothing lists objects in one place. `Document.objects` is a public ordered map. Every feature loops over it alone.
- The scan found 247 production functions with 288 loops over the whole map. 95 of them need a proposal rule. 100 belong to the engine and see everything. 52 are not listings of the document.
- Three mechanisms exist that are close to the need. They are the Hide flag of MOD-05, the export copy `leftout::prepare` and the `visible_only` option of the exporters. The design reuses all three.
- The filter lands before proposals exist. Then every object has no proposal field. So each stage changes no behavior and merges alone.
- The design has a typed field `Object.proposal` and a `Purpose` enum. It adds `Document::objects_for(purpose)`, a scan test with an allow list, and at the end a private `objects` field.
- Five places carry most of the risk. They are the revision-keyed caches and the site and terrain chain. The others are the export entry points beside `export_impl`, the agent list defaults and the references between objects.
- The owner has 13 decisions. Section 6 gives a recommendation for each.

## 2. How objects are listed today

### 2.1 The store

- `Document.objects` is `IndexMap<ObjectId, Object>` (`kadakas-core/src/model.rs:1147` and `:1189`). The field is `pub`.
- The map order is the stacking order. A filter that keeps the order keeps the stacking.
- A layer holds no list. An object holds its `layer`, `class` and `parent` (`model.rs:933`). So `layer.objects` does not exist. A listing by layer is a filter over the whole map.
- `Object` has a flatten map `extra`. A build that does not know a field reads it into `extra` and writes it back (`forward_compat.rs`).
- `Document::object(id)` is the lookup by id (`model.rs:1976`). A lookup by id is not a listing. It needs no proposal rule.
- The scan found 516 text hits of `.objects` in production code. 288 are loops over the whole map. About 80 are lookups, counts, writes or indexes. About 150 belong to other structs with a field of the same name (issues, step data, reports, DXF files, scenes).
- Test code has about 1000 more hits. The scan test of section 4.3 skips test code.

### 2.2 Filters that exist now

| Filter | Where | Calls. |
|---|---|---|
| `is_visible`: layer and class visible, not hidden | `model.rs:2003` | 23. |
| `is_displayed`: visible and not hidden by phase rules | `model.rs:2019` | 9. |
| `is_pickable` | `model.rs:2025` | 3. |
| `is_shown`: any layer kind | `model.rs:2034` | 5. |
| `is_hidden`: the Hide flag | `visibility.rs:56` | 11. |
| `Viewport::shows`: layers and classes of one viewport | `sheets.rs:796` | 13. |

Most reports apply no filter. The plant schedule and the area reports loop over all objects. They count objects on hidden layers too. The comment at `model.rs:2017` says that calculations use `is_visible`. Only 23 sites do.

So the proposal filter is a new rule, not a change of an old one. It reaches sites that have no filter today.

### 2.3 Precedents to reuse

1. **Hide (MOD-05)**. The flag is `data["kadakas.hidden"]` with the reserved prefix `kadakas.` (`visibility.rs:49`). Views, picking and exports leave hidden objects out. An export says how many.
2. **Export copy**. `leftout::prepare` (`leftout.rs:114`) clones the document without guides and hidden objects. `Session::export_impl` (`kadakas-api/src/io.rs:234`) runs every `export.file` format on that copy. The answer has `left_out` and `warnings`. The clone happens only for a document that has something to leave out.
3. **`visible_only`**. The exporters take this option (`kadakas-io/src/dxf/export.rs:422`, `geojson.rs:329`, `kadakas-ifc/src/export.rs:633`). It is the model for `include_proposals`.
4. **`model_only`**. `kadakas-io/src/lib.rs:279` returns `Cow<Document>`. The copy is free for a document without sheet layers.
5. **A source gate with an allow list**. `scripts/check_number_format.py` scans the UI crates and reads `scripts/number-format-allow.txt`. A stale line fails the run. CI runs it (`.github/workflows/ci.yml:188`).
6. **Unknown data survives**. `forward_compat.rs` tests that unknown object fields and shape kinds are saved again. MOD-15 writes a model as version 2 only for a model that uses the new field.
7. **Compare overlay**. `CompareOverlay` (`kadakas-review/src/overlay.rs`) has the statuses added, removed and changed (`Status`, line 62). The fourth colour of REV-12 item 6 goes there.

Hide is not enough for proposals. A hidden object is drawn nowhere. A proposal is drawn, selected and measured.

### 2.4 How exports reach the files

- `export.file` goes through `export_impl` for every format. That covers DXF, DWG, IFC, glTF, GLB, OBJ, STL, GeoJSON, Shapefile, PDF and the sheet formats. The app file dialog and the CLI (`kadakas-cli/src/main.rs:883`) use the same path.
- STEP is read only today (`kadakas-step`). A future STEP writer joins the same path.
- The exporters are public functions: `kadakas_io::export_file`, `kadakas_ifc::export_to`, `kadakas_x3d::export_gltf`, `kadakas_print::export_pdf`. A caller that skips `export_impl` skips the copy. The types do not stop it.
- Two paths skip it today. File > Export SVG calls `Session::render_svg` (`kadakas-api/src/lib.rs:1228`, used by `kadakas-app/src/file_io.rs:335`). The web build calls `kadakas_core::svg::render` (`kadakas-app/src/app.rs:1383`).
- The 3D exports build the scene from the copy with `kadakas_core::mesh::build_with_detail`. Its private helper `visible_objects` (`mesh.rs:1829`) is the one place that lists objects for 3D.
- `Session::scene()` (`kadakas-api/src/lib.rs:1395`) caches the scene by the editor revision. Sections, the site cache and the terrain index use the same key.

### 2.5 The surfaces outside Rust

Python, JavaScript, MCP and the wasm plug-ins have no listing of their own. They all call `Session::handle`. Their rule is the rule of the API method.

| Surface | Entry | Rule | What it lists. |
|---|---|---|---|
| `objects.list` | `kadakas-api/src/lib.rs:1772` | XO | A filter over the map. |
| `objects.get` | `lib.rs:1797` | IN | One id. |
| `objects.query` | `query_api.rs:183` | XO | Criteria over the map, through `kadakas-sheet/src/db.rs:311`. |
| `document.summary` | `lib.rs:1677` | XO | Count, count by kind, bounds. Plus a proposal count. |
| `selection.*` | `lib.rs:1822` | IN | Ids and hit tests. |
| `report.*` | `lib.rs:2104`, `plants_api.rs:387` | XO | Plants, areas, schedule, trees, hardscape. |
| `massing.report` | `massing_api.rs:390` | XO | Totals of massing models. |
| `unit_pattern.report` | `pattern_api.rs:1109` | XO | Unit counts per hardscape. |
| `site.*`, `irrigation.calculate` | `irrigation_api.rs:204` | XO | Calculations. |
| `snap.query`, `snap.smart` | `query_api.rs:255` | XO | Snap targets. |
| `mesh.*` | `mesh_api/mod.rs:465` | XO | Surface models. |
| `render.*`, `sheet.render` | `lib.rs:1880` | IN | The picture, in proposal style. |
| `export.file` | `kadakas-api/src/io.rs:234` | EC | The file. |
| `version.*`, `issue.*`, `collab.*` | `review_api.rs` | IN | The whole document or ids. |
| Python `Document.objects()` | `python/kadakas/document.py:888` | XO | Calls `objects.list`, then filters with `where` on the client. |
| JavaScript, Python scripts, plug-ins, MCP | `kadakas-script*`, `kadakas-cli/src/mcp_tools.rs` | XO | Forward `{method, params}`. |

The keys of the Rule column are in section 3.3. MCP in the flat mode lists each method with the schema of its params struct. A new `include_proposals` field shows in MCP, in the Python types and in `describe` with no extra work.

## 3. The survey

### 3.1 Method and limits

- A script walked `crates/*/src` outside `#[cfg(test)]` modules and test files. It matched `.objects`, also where `.objects` stands alone on a line. A second pass classified each hit by hand from the code around it.
- The scan counts a hit as a loop for these calls on the map: `values`, `iter`, `keys`, `values_mut`, `iter_mut`, `retain`, `drain`. A `for` loop counts too.
- Loops inside `Document` helpers count in `kadakas-core` (`group.rs`, `guide.rs`, `sheets.rs`). The callers of those helpers are not counted again.
- Scenes are one source. The scene builder lists objects once. Code that reads `scene.objects()` inherits the choice made there. Those readers are marked "none".
- A listing that copies the map first (`let m = &doc.objects`) counts as one hit. A hand review covered the 38 hits that pass the map on as a value.
- A few test functions live outside a `mod tests` block (in `kadakas-app` and `kadakas-ifc`). They show up as "none".

### 3.2 Counts per crate

```
crate                 functions  sites  need-rule  engine  none  other-access
kadakas-annot                 4      4          4       0     0             0
kadakas-api                  51     58         20      25     6             9
kadakas-app                  11     14          4       2     5             4
kadakas-core                 90    112         24      53    13            35
kadakas-file                  1      1          0       1     0             2
kadakas-gis                   1      2          0       1     0             0
kadakas-ifc                   6      9          2       1     3             4
kadakas-io                   18     20         10       0     8            17
kadakas-irrigation            2      2          2       0     0             0
kadakas-modeler               3      3          2       0     1             0
kadakas-plants                3      3          3       0     0             0
kadakas-print                 4      4          3       1     0             0
kadakas-props                 6      6          0       6     0             2
kadakas-render                2      2          1       0     1             2
kadakas-review                9     11          0       5     4             3
kadakas-sheet                 2      2          2       0     0             0
kadakas-site                  6      6          6       0     0             0
kadakas-ui-core              27     28         12       5    10             1
kadakas-x3d                   1      1          0       0     1             1
TOTAL                       247    288         95     100    52            80
```

- "Functions" is the number of functions with a loop over the map. "Sites" is the number of loops in them.
- "Need-rule" is the number of rows in Table A. "Engine" is Table B. "None" is Table C.
- "Other-access" is lookups by id, counts, writes and indexes. They stay as they are. Writes move behind `all_objects_mut()` in the last stage.
- By rule: 6 always exclude, 43 exclude with an option, 23 include, 13 by purpose, 10 export copy.
- Effort in Table A: 60 small, 31 medium, 4 large.

### 3.3 Keys for the tables

| Key | Rule | Meaning. |
|---|---|---|
| AX | always exclude | A proposal never counts. No option. |
| XO | exclude, option | A proposal does not count. The call takes `include_proposals`. |
| IN | include | A proposal is part of the result. Drawing uses the proposal style. Selection follows the mode. |
| BP | by purpose | One function serves two purposes. Drawing includes proposals. Calculation leaves them out. |
| EC | export copy | The code runs on the copy of `leftout::prepare`. It needs no change itself. |
| engine | all objects | Save, load, undo, validation, sync, usage counts and limits. |
| none | not a listing | The hit is not a loop over the document. |

Effort: S is one call swapped. M needs an option in a params struct, a signature change or a cache key. L needs design or reaches many callers.

### 3.4 Table A: sites that need a rule

| Function | File | Rule | Eff | Listing for. |
|---|---|---|---|---|
| `annot::select` | `kadakas-annot/src/lib.rs:118` | XO | S | criteria select for tags and legends (Tag). |
| `annot::legend_rows` | `kadakas-annot/src/lib.rs:155` | XO | S | legend rows: counts per criteria (Legend). |
| `annot::tag_all` | `kadakas-annot/src/tags.rs:26` | AX | S | auto data tags: tag every matching object (Tag). |
| `annot::revisions` | `kadakas-annot/src/tags.rs:80` | XO | S | revision list of clouds (Report). |
| `api::list` | `kadakas-api/src/annotation_api.rs:862` | XO | S | annotation.list for agents (Search). |
| `api::rows` | `kadakas-api/src/bed_api.rs:228` | XO | S | bed.* list rows (Search). |
| `api::handle_bed` | `kadakas-api/src/bed_api.rs:257` | XO | S | bed.* handler finds beds (Search). |
| `api::run` | `kadakas-api/src/data_api.rs:675` | XO | M | data.* record evaluation and worksheet table sync (Calculate). |
| `api::view_of` | `kadakas-api/src/gis_api.rs:703` | XO | S | map view box of the drawing (Calculate). |
| `api::wall_report` | `kadakas-api/src/grading_api.rs:638` | XO | S | retaining wall report (Calculate). |
| `api::spot_get` | `kadakas-api/src/grading_api.rs:985` | XO | S | spot_get: first site model (Calculate). |
| `api::section_place` | `kadakas-api/src/grading_api.rs:1120` | XO | S | section_place: stale profile objects (Calculate). |
| `api::modifier_warning` | `kadakas-api/src/grading_api/retaining.rs:402` | XO | S | retaining modifier warning: any site model (Check). |
| `api::analysis` | `kadakas-api/src/irrigation_api.rs:204` | XO | S | irrigation analysis: has a site model (Calculate). |
| `api::current_view` | `kadakas-api/src/issue_api.rs:504` | IN | S | frame of the current view (Draw). |
| `api::dispatch` | `kadakas-api/src/lib.rs:1680` | XO | M | document.summary counts, objects.list, report.areas, bounds (Search, Calculate). |
| `api::report` | `kadakas-api/src/massing_api.rs:390` | XO | S | massing.report: area and volume totals (Calculate). |
| `api::plan_notes` | `kadakas-api/src/mesh_api/mod.rs:336` | IN | S | plan notes for reduced models (Draw). |
| `api::models` | `kadakas-api/src/mesh_api/mod.rs:465` | XO | M | mesh.* list of surface models (Search). |
| `api::summary` | `kadakas-api/src/mesh_api/read.rs:130` | XO | S | mesh.summary counts of components (Search). |
| `api::export_stl_file` | `kadakas-api/src/meshio_api.rs:1076` | EC | S | STL export (covered by export prepare). |
| `api::report_rows` | `kadakas-api/src/pattern_api.rs:1109` | XO | S | unit_pattern report rows and totals (Calculate). |
| `api::check_call` | `kadakas-api/src/plan_api.rs:1692` | XO | S | unit_pattern.check default set (Check). |
| `api::phase_report` | `kadakas-api/src/rules_api.rs:280` | XO | S | phase report: counts by phase (Calculate). |
| `app::select_all` | `kadakas-app/src/app.rs:1182` | IN | M | select all (Select). |
| `app::paint` | `kadakas-app/src/canvas.rs:675` | IN | M | paint the plan and the selection halo (Draw). |
| `app::draw_preview` | `kadakas-app/src/canvas.rs:925` | IN | S | paint a drag preview of given ids (Draw). |
| `app::collect` | `kadakas-app/src/plant_sheet_ui.rs:313` | XO | S | plants of a style in the plant sheet (Calculate). |
| `core::other_chains` | `kadakas-core/src/edit2d/plan.rs:89` | XO | M | trim, extend and offset: chains that cut a target (Snap). |
| `core::report` | `kadakas-core/src/hardscape/report.rs:250` | XO | S | hardscape report: areas and volumes (Calculate). |
| `core::report` | `kadakas-core/src/landscape/mod.rs:637` | XO | S | `LandscapeArea` report (Calculate). |
| `core::of` | `kadakas-core/src/leftout.rs:39` | EC | M | counts what an export leaves out (extend with proposals). |
| `core::prepare_with` | `kadakas-core/src/leftout.rs:126` | EC | M | builds the export copy (extend with proposals). |
| `core::visible_objects` | `kadakas-core/src/mesh.rs:1830` | BP | L | 3D scene: objects with geometry (Draw). |
| `core::count_detail` | `kadakas-core/src/mesh.rs:1875` | IN | S | plant detail level by count (Draw). |
| `core::mean_plant_spread` | `kadakas-core/src/mesh.rs:1891` | IN | S | plant spread for level of detail (Draw). |
| `core::hydrozones` | `kadakas-core/src/plants/care.rs:181` | XO | S | hydrozones: water demand per zone (Calculate). |
| `core::tag_counts` | `kadakas-core/src/plants/tag.rs:173` | XO | S | plant tag counts per style and layer (Calculate). |
| `core::new` | `kadakas-core/src/present.rs:860` | IN | S | material slots for painting (Draw). |
| `core::spread_group_materials` | `kadakas-core/src/present.rs:887` | IN | S | material spread over groups (Draw). |
| `core::view_warnings` | `kadakas-core/src/sheets.rs:694` | IN | S | viewport warnings for underlays (Draw). |
| `core::content_bounds` | `kadakas-core/src/sheets.rs:853` | BP | M | viewport content bounds (Draw, follows shows). |
| `core::fit` | `kadakas-core/src/sheets.rs:877` | BP | M | viewport fit (Draw, follows shows). |
| `core::site_layers` | `kadakas-core/src/site.rs:38` | XO | S | layers that have a site model (Calculate). |
| `core::from_doc` | `kadakas-core/src/snapping.rs:364` | XO | M | snap geometry near the pointer (Snap). |
| `core::drawing_bounds` | `kadakas-core/src/svg.rs:17` | IN | S | bounds of visible objects (Draw). |
| `core::render_box` | `kadakas-core/src/svg.rs:266` | IN | M | 2D SVG picture (Draw, export uses the copy). |
| `core::beds_for` | `kadakas-core/src/texture_bed.rs:222` | BP | M | texture beds that act on a site (Draw and Calculate). |
| `core::all_beds` | `kadakas-core/src/texture_bed.rs:271` | BP | M | texture beds of every site (Draw and Calculate). |
| `core::site_mapping` | `kadakas-core/src/texture_bed.rs:284` | BP | M | texture mapping of the first site (Draw and Calculate). |
| `core::auto_rules` | `kadakas-core/src/viewrules.rs:561` | XO | S | visualization rules from values (Search). |
| `core::legend` | `kadakas-core/src/viewrules.rs:657` | XO | S | visualization legend counts (Calculate). |
| `ifc::run` | `kadakas-ifc/src/export.rs:272` | EC | S | IFC export loop (covered by export prepare). |
| `ifc::site_surface_shape` | `kadakas-ifc/src/export.rs:633` | EC | S | IFC site surface (covered by export prepare). |
| `io::export` | `kadakas-io/src/dxf/export.rs:422` | EC | S | DXF export loop (covered by export prepare). |
| `io::export` | `kadakas-io/src/dxf/export/r12.rs:345` | EC | S | DXF R12 export loop (covered by export prepare). |
| `io::write_sheet` | `kadakas-io/src/dxf/export/sheets.rs:454` | EC | S | DXF sheet export (covered by export prepare). |
| `io::drawings` | `kadakas-io/src/dxf/update.rs:121` | AX | M | update from DXF: imported drawings (Official). |
| `io::plan` | `kadakas-io/src/dxf/update.rs:683` | AX | M | update from DXF: plan against tags (Official). |
| `io::collect` | `kadakas-io/src/dxf/update/site.rs:79` | AX | S | update from DXF: site sources (Official). |
| `io::count` | `kadakas-io/src/dxf/update/site.rs:126` | AX | S | update from DXF: source counts (Official). |
| `io::homes` | `kadakas-io/src/dxf/update/site.rs:146` | AX | S | update from DXF: site homes (Official). |
| `io::export` | `kadakas-io/src/geojson.rs:329` | EC | S | GeoJSON export loops (covered by export prepare). |
| `io::model_only` | `kadakas-io/src/lib.rs:291` | EC | S | model_only copy for exporters (central place). |
| `irrigation::extract_with` | `kadakas-irrigation/src/extract.rs:304` | XO | M | irrigation network extraction (Calculate). |
| `irrigation::zone_commands` | `kadakas-irrigation/src/lib.rs:32` | XO | S | irrigation zone commands (Calculate). |
| `modeler::run` | `kadakas-modeler/src/app.rs:781` | IN | S | top-level objects of the modeler (Select). |
| `modeler::intersect_models` | `kadakas-modeler/src/app.rs:910` | IN | S | models for Intersect Faces (Select). |
| `plants::plant_list` | `kadakas-plants/src/plant_list.rs:104` | XO | S | plant list (Calculate). |
| `plants::schedule` | `kadakas-plants/src/schedule.rs:54` | XO | S | plant schedule (Calculate). |
| `plants::existing_trees` | `kadakas-plants/src/schedule.rs:207` | XO | S | existing tree schedule (Calculate). |
| `print::compose_page_limited` | `kadakas-print/src/compose.rs:145` | BP | M | sheet page: objects of a sheet layer (Draw, PDF uses the copy). |
| `print::plan_view` | `kadakas-print/src/compose.rs:262` | BP | M | sheet viewport content (Draw, PDF uses the copy). |
| `print::viewport_document` | `kadakas-print/src/compose.rs:303` | BP | M | viewport document for the 3D renderer (Draw). |
| `render::patterned_untextured` | `kadakas-render/src/look.rs:195` | IN | S | count of hardscapes with untextured joints (Draw). |
| `sheet::select` | `kadakas-sheet/src/db.rs:231` | XO | M | worksheet database: select rows (Calculate). |
| `sheet::query_objects` | `kadakas-sheet/src/db.rs:311` | XO | M | worksheet and objects.query: find by criteria (Search). |
| `site::scene_ext` | `kadakas-site/src/draw.rs:242` | BP | L | site model scene extension (Draw, Calculate). |
| `site::extras_at` | `kadakas-site/src/grading/hook.rs:70` | BP | M | grading extras at a scale (Draw). |
| `site::host` | `kadakas-site/src/grading/mod.rs:142` | XO | M | host site model of a layer (Calculate). |
| `site::seen` | `kadakas-site/src/grading/section.rs:266` | BP | M | objects that a section sees (Draw and Calculate). |
| `site::modifiers_for` | `kadakas-site/src/model.rs:80` | XO | L | modifiers that act on a site (Calculate). |
| `site::new` | `kadakas-site/src/terrain.rs:92` | BP | L | terrain index over site models (Calculate). |
| `ui-core::hit_object` | `kadakas-ui-core/src/hit.rs:207` | IN | M | hit test: topmost object (Select). |
| `ui-core::hit_test_in` | `kadakas-ui-core/src/hit.rs:233` | IN | M | hit test inside a group (Select). |
| `ui-core::box_select_in` | `kadakas-ui-core/src/hit.rs:264` | IN | M | box select (Select). |
| `ui-core::lasso_select_in` | `kadakas-ui-core/src/hit.rs:353` | IN | M | lasso select (Select). |
| `ui-core::fill_faces_checked` | `kadakas-ui-core/src/modeling/paint.rs:211` | IN | S | paint bucket over visible models (Select). |
| `ui-core::get` | `kadakas-ui-core/src/modeling/scenes.rs:195` | IN | M | surface scenes of models (Draw). |
| `ui-core::rect_select` | `kadakas-ui-core/src/modeling/select.rs:423` | IN | S | rectangle select of faces and edges (Select). |
| `ui-core::rect_select_objects` | `kadakas-ui-core/src/modeling/select.rs:486` | IN | S | rectangle select of objects (Select). |
| `ui-core::targets` | `kadakas-ui-core/src/paving/check.rs:215` | XO | S | paving check: all hardscapes (Check). |
| `ui-core::geometry` | `kadakas-ui-core/src/snap.rs:101` | XO | S | snap geometry: irrigation objects (Snap). |
| `ui-core::rows` | `kadakas-ui-core/src/tags.rs:64` | XO | S | tag rows with counts (Calculate). |
| `ui-core::anchor_at` | `kadakas-ui-core/src/tools/annot_tool.rs:197` | XO | S | annotation anchor snap (Snap). |

### 3.5 Table B: engine and structure

These sites see every object on purpose. In the last stage each moves to `all_objects()`. Each entry is a function and the first line of its loop. The allow list of the scan test holds them.

- `kadakas-api/src/annotation_api.rs`: `style_json` 331.
- `kadakas-api/src/collab_api.rs`: `stamp_keys` 831.
- `kadakas-api/src/data_api.rs`: `format_json` 269.
- `kadakas-api/src/feedback_api.rs`: `near_middle` 622.
- `kadakas-api/src/feedback_api/excerpt.rs`: `build` 73, `center` 467, `objects_of` 648.
- `kadakas-api/src/grading_api.rs`: `used_by` 275, `wall_style_delete` 478.
- `kadakas-api/src/hardscape_api.rs`: `used_by_hardscape` 227, `used_by_landscape` 234.
- `kadakas-api/src/limits.rs`: `document_bytes` 229.
- `kadakas-api/src/massing_api.rs`: `follow_sources` 335.
- `kadakas-api/src/mesh_api/edit.rs`: `of` 651, `item_json` 679.
- `kadakas-api/src/meshio_api.rs`: `import_mesh_file` 635.
- `kadakas-api/src/plants_api.rs`: `style_json` 156, `drape` 598, `ground_height_note` 672.
- `kadakas-api/src/plugin_api.rs`: `instances_of` 245.
- `kadakas-api/src/review_api.rs`: `restore_version` 1135.
- `kadakas-api/src/sheets_api.rs`: `sheet_json` 353, `viewport_list` 441.
- `kadakas-api/src/wasm_plugin_api.rs`: `usage` 175, `instances_of` 186.
- `kadakas-app/src/data_ui.rs`: `record_formats` 498.
- `kadakas-app/src/plant_ui.rs`: `used_by` 252.
- `kadakas-core/src/classes.rs`: `uses` 61.
- `kadakas-core/src/command.rs`: `forget_site_names` 1351, `remove_objects` 1472, `replace_content` 1480, `run` 1968, `cap_exact` 2346, `replace_missing_pictures_with_undo` 2855.
- `kadakas-core/src/command/annot_ops.rs`: `annot_after` 82, `annot_sync` 140.
- `kadakas-core/src/command/component_ops.rs`: `edit_component` 729.
- `kadakas-core/src/command/drawing_ops.rs`: `check_sheet_rules` 118, `run_drawing` 237.
- `kadakas-core/src/command/graphic_ops.rs`: `check_instance_kinds` 86.
- `kadakas-core/src/command/group_ops.rs`: `with_group_members` 231, `edit_symbol` 667.
- `kadakas-core/src/command/plant_ops.rs`: `check_resource_name` 87, `users_of` 132.
- `kadakas-core/src/command/record_ops.rs`: `record_users` 23.
- `kadakas-core/src/command/retaining_ops.rs`: `retaining_after` 23.
- `kadakas-core/src/command/steps.rs`: `dependants` 781.
- `kadakas-core/src/component.rs`: `raw_model` 751, `parts_from_objects` 771, `instance_findings` 1081, `of` 1142, `biggest_user` 1171.
- `kadakas-core/src/datamap.rs`: `record_commands` 448.
- `kadakas-core/src/group.rs`: `children` 28, `descendants` 42, `leaves` 105, `with_members` 121, `check_tree` 137.
- `kadakas-core/src/guide.rs`: `guides` 157.
- `kadakas-core/src/library.rs`: `usage` 121, `list` 182.
- `kadakas-core/src/model.rs`: `eq` 1242, `for_each_blob` 1496, `placeholder_hashes` 1596, `relink_placeholders_noted` 1643, `replace_missing_pictures` 1709, `use_sequential_ids_after_existing` 1811, `validate` 1858.
- `kadakas-core/src/present/paint.rs`: `material_use_map` 150.
- `kadakas-core/src/props.rs`: `hardscape_name_notes` 286.
- `kadakas-core/src/provenance.rs`: `base_users` 590, `repair` 806.
- `kadakas-core/src/sheets.rs`: `viewports_of` 381.
- `kadakas-core/src/symbols.rs`: `is_used_by_objects` 555, `repair_edits` 565, `instance_ids` 706.
- `kadakas-core/src/visibility.rs`: `hidden_object_count` 79, `unhide_all_command` 92, `hidden_objects` 120.
- `kadakas-core/src/worksheet_table.rs`: `users_of` 57.
- `kadakas-file/src/write.rs`: `estimate_document_bytes` 555.
- `kadakas-gis/src/reproject.rs`: `plan` 393.
- `kadakas-ifc/src/import/props.rs`: `write_records` 132.
- `kadakas-print/src/table.rs`: `build` 81.
- `kadakas-props/src/docsync.rs`: `all_targets` 118, `carried_keys` 468, `lists_keys` 502, `order_ops` 550, `ops_from_change` 763.
- `kadakas-props/src/materialize.rs`: `blame` 400.
- `kadakas-review/src/restore.rs`: `without_stacking_keys` 75, `copy` 96, `align_global_order` 120.
- `kadakas-review/src/state.rs`: `stamp_keys` 194, `stamp_order` 229.
- `kadakas-ui-core/src/grading.rs`: `has_site` 100.
- `kadakas-ui-core/src/modeling/exec.rs`: `run_full` 237.
- `kadakas-ui-core/src/modeling/resize.rs`: `resize_plan` 44.
- `kadakas-ui-core/src/style_edit.rs`: `note_warnings` 402.
- `kadakas-ui-core/src/tools/irrigation_tool.rs`: `next_zone` 109.

### 3.6 Table C: not listings of the document

These hits are fields of other structs, test code or temporary documents. They need no change.

- `kadakas-api/src/dxf_compare_api.rs`: `with_clouds` 242.
- `kadakas-api/src/feedback_api/excerpt.rs`: `assemble` 155, `apply_privacy` 324.
- `kadakas-api/src/issue_api.rs`: `full` 208.
- `kadakas-api/src/review_api/props.rs`: `ids` 155, `run` 267.
- `kadakas-app/src/agent_activity_ui.rs`: `objects` 741.
- `kadakas-app/src/issues_ui.rs`: `detail_ui` 709.
- `kadakas-app/src/paving_ui/windows.rs`: `check_window` 535.
- `kadakas-app/src/surface_draw.rs`: `the_cache_drops_the_plans_that_were_used_least_and_keeps_the_rest` 896, `time_of_5000_instances` 963.
- `kadakas-core/src/activity.rs`: `of` 58, `ids` 269, `object_ids` 282.
- `kadakas-core/src/command.rs`: `objs` 3118.
- `kadakas-core/src/command/component_ops.rs`: `make_group` 185, `make_component` 249.
- `kadakas-core/src/command/graphic_ops.rs`: `graphic_delete` 205.
- `kadakas-core/src/command/steps.rs`: `object_counts` 95, `inverted` 123, `approx_bytes` 145, `revert_conflicts` 489, `revert_step` 634.
- `kadakas-core/src/issue.rs`: `validate` 548.
- `kadakas-ifc/src/import/tests_more.rs`: `objs` 61.
- `kadakas-ifc/src/import/tests_real.rs`: `every_kind_of_object_far_from_the_origin_is_imported_near_it` 276, `points_far_from_the_origin_find_the_file_when_every_placement_is_at_the_origin` 414.
- `kadakas-io/src/dwg.rs`: `imports_through_libredwg` 785.
- `kadakas-io/src/dxf/block.rs`: `block_outlines` 120.
- `kadakas-io/src/dxf/compare.rs`: `compare` 200.
- `kadakas-io/src/dxf/import/extra.rs`: `image_reason` 339.
- `kadakas-io/src/dxf/import/paper.rs`: `layout_of` 37, `layouts` 125.
- `kadakas-io/src/report.rs`: `total` 80.
- `kadakas-io/src/shapefile.rs`: `import_through_the_trait` 684.
- `kadakas-modeler/src/edit3d.rs`: `refresh_extras` 225.
- `kadakas-render/src/hlr.rs`: `crowded_objects` 553.
- `kadakas-review/src/bcf/read.rs`: `same_content` 960.
- `kadakas-review/src/bcf/write.rs`: `out_viewpoints` 203.
- `kadakas-review/src/issues.rs`: `pin_position` 180.
- `kadakas-review/src/testutil.rs`: `run` 78.
- `kadakas-ui-core/src/agent_review.rs`: `build` 77, `step_bounds` 234.
- `kadakas-ui-core/src/issues.rs`: `open_plan` 466.
- `kadakas-ui-core/src/modeling/xform.rs`: `resolve` 149, `measure` 278, `outline` 319, `models` 356.
- `kadakas-ui-core/src/paving/check.rs`: `findings` 187, `finding_count` 191, `errors` 195.
- `kadakas-x3d/src/parts.rs`: `build` 326.

### 3.7 What the survey says

1. **Reports and schedules are cheap**. The plant schedule, plant list, area reports, massing report and tag counts are single loops. Each swaps in one line.
2. **The worksheet database is one choke point**. `kadakas-sheet/src/db.rs:231` and `:311` serve the RPT worksheets, `objects.query` and the legend selectors. One change there covers all worksheet columns.
3. **Site and terrain are the hard part**. Five functions feed volumes, contours, plant heights and the 3D surface (section 5, risk 2). They need the purpose.
4. **Selection and snapping are local**. About ten places list objects in `hit.rs`, `snap.rs`, `snapping.rs` and the modeler. They take the purpose from the view state.
5. **Exports are mostly done**. Seven export functions sit behind `export_impl`. The gaps are the SVG menu command and the public exporter functions.
6. **The engine is half of the map**. About 100 functions need every object. Their allow list is the price of a gate that is strict everywhere else.

## 4. Design

### 4.1 (a) How a proposal is stored

**Field**. `Object` gets one typed field next to `parent`:

```
pub struct Object {
    id, layer, class, shape, attrs, data, parent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<Box<Proposal>>,
    extra,
}

pub struct Proposal {
    pub group: Option<ResourceId>,
    pub author: String,
    pub origin: Origin,
    pub status: ProposalStatus,
    pub note: String,
    pub time: i64,
    pub replaces: Option<ObjectId>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}
```

- `Origin` is the existing enum of `change.rs:23` (user, agent, script, remote).
- The JSON key is `proposal`. A file without proposals has no key and is unchanged.
- The `Box` keeps `Object` small for the common case. A `None` is one machine word.
- `ProposalStatus` stores `open`, `needs_work` and `rejected`. It has an `Other(String)` value for a status that a newer build writes. Accepted is not stored. An accepted proposal has no field.
- The filter reads `proposal.is_some()`. It never reads the status. An unknown status, a rejected proposal and an open one are all out of the official totals. This fails closed.
- `replaces` marks a proposed change of an existing object. The original stays untouched.

**Why a field and not `data["kadakas.proposal"]`**.

| | Field | Key in `data`, like Hide. |
|---|---|---|
| Cost of the filter | One pointer test | A map lookup and a JSON parse. |
| Reaches exports and worksheets | No | Yes, unless each reader skips the prefix. |
| Struct change | About 40 literals, mostly tests | None. |
| Sync | One property `proposal` | The property `data.kadakas.proposal`. |
| Typed code | Yes | No, strings. |

`ifc/export/finish.rs:190` walks `obj.data.keys()`, which shows how a data key reaches a file. The recommendation is the field (D2).

**Class `Proposal`**. A system class gives layers, classes and visibility tools their handle (REV-12 item 1). There is no system class mechanism today beyond `NONE_CLASS`, a well-known ID. The design adds a second well-known class ID. A document creates it with the first proposal. It joins the protected things of collaboration section 3.3.

The class is presentation only. The field is the truth. An object moved into the class `Proposal` does not become a proposal. A proposal moved to another class does not become official.

**Old readers and the file format**.

- The tolerant reader rule holds for every build. A build that does not know the key reads it into `extra` and writes it back. It does not drop it. Inside `Proposal`, `extra` and `ProposalStatus::Other` keep what a newer build adds.
- A build from before proposals that opens the file shows each proposal as an official object. Its totals include them. The tolerant rule does not prevent this.
- So the design offers a fail-closed option. Write `format_version` 4 only for a document that holds a proposal. MOD-15 does the same with its version 2 models. An old build refuses the file with "made by a newer Kadakas (format 4)" (`LoadError::Newer`). A file without proposals stays at 3.
- This is decision D1. The cost is that a colleague with an old build is unable to open a file with proposals.

**Collaboration**. The property model needs no new operation.

- An object property is "any other top-level field" (`docs/collaboration.md` section 3.1). `proposal` is one property. `flatten` in `kadakas-props/src/docsync.rs:131` passes it through.
- Create a proposal: a `create` with the property `proposal` in `props`. Accept: `unset proposal`. Reject: `set proposal` with status `rejected`.
- The value is one property with last writer wins. An accept is one atomic `unset`. A note edit that races with an accept loses. Notes live in the REV-04 comment thread, so this is rare.
- The alternative is to cut the value into `proposal.status` and `proposal.note`, like `attrs.<field>`. It costs two more match arms in `flatten` and `unflatten` and gives no atomic accept. D3 records the choice.
- `kadakas-props/src/validate.rs` and `materialize.rs` get a check. A `proposal` with an unknown group becomes an ungrouped proposal. A value that fails to parse goes to quarantine, like any value that fails validation.
- An old client in a shared document keeps the key in `extra` and counts the proposal as official. The `hello` message has `protocol: 1`. The server of PLAT-09 refuses old clients for a document with proposals. That work is in `kadakas-server`. D1 lists it.
- A named version (REV-01) is a set of property states. It keeps `proposal` with no change. A restore brings proposals back as they were.

### 4.2 (b) The central API

```
pub enum Purpose {
    Everything,
    Official,
    Calculate(CalcKind),
    Check,
    Tag,
    Search,
    Snap,
    Select,
    Draw(DrawKind),
    Export(ExportKind),
    Compare,
}

pub enum CalcKind { Quantity, Schedule, PlantCount, Irrigation, Hydraulic, Area, Volume, Cost, Terrain }
pub enum DrawKind { Plan, Scene, Sheet, Picture }
pub enum ExportKind { Ifc, Dxf, Dwg, Gltf, Obj, Stl, Step, GeoJson, Shapefile, Pdf, Svg, Sheet, Publish }

pub enum Rule { Include, Exclude, ExcludeUnlessAsked }

impl Purpose {
    pub const fn rule(self) -> Rule
}

pub struct ProposalOpts {
    pub include: bool,
    pub shown: Shown,
}

impl Document {
    pub fn objects_for(&self, p: Purpose) -> ObjectsFor<'_>
    pub fn objects_for_with(&self, p: Purpose, o: &ProposalOpts) -> ObjectsFor<'_>
    pub fn all_objects(&self) -> impl DoubleEndedIterator<Item = &Object>
    pub fn all_objects_mut(&mut self) -> impl Iterator<Item = &mut Object>
    pub fn proposal_count(&self) -> usize
}
```

The matrix of rules:

| Purpose | Rule | Option. |
|---|---|---|
| `Everything` | include | None. Only the files of the allow list use it. |
| `Official` | exclude | `include_proposals`. |
| `Calculate(_)` | exclude | `include_proposals`. |
| `Check` | exclude | `include_proposals`. |
| `Tag` | exclude | None. |
| `Search` | exclude | `include_proposals`. |
| `Snap` | exclude | `snap.set {proposals: true}` and a toggle in the snap bar. |
| `Select` | include, per view state | The Proposal mode and the Proposals palette. |
| `Draw(_)` | include, in proposal style | The switcher of item 8 picks the options that show. |
| `Export(Publish)` | exclude | None. A published revision (PLAT-30) is always official. |
| `Export(_)` | exclude | `include_proposals`. |
| `Compare` | include on purpose | The totals of the compare view in item 8. |

Details:

- `Purpose::rule` is a `const fn` with an exhaustive `match` and no wildcard. A new purpose does not compile until it has a rule.
- `ObjectsFor` is a struct around `indexmap::map::Values` and one predicate. It implements `Iterator` and `DoubleEndedIterator`. Hit testing needs `.rev()` for top-down order (`kadakas-ui-core/src/hit.rs:207`).
- `ProposalOpts::shown` is the view state of the switcher: the groups that show. It is a personal view setting and is not shared (REV-12 item 8). `Draw` and `Select` read it. The other purposes ignore it.
- A call with explicit ids is not a listing and passes no filter. `objects.get`, `selection.set` and the `ids` parameters of the reports work on named proposals.
- A listing that leaves proposals out reports the count. `objects.list`, `document.summary` and the reports return `proposals_hidden: n` for n above zero. Nothing is dropped silently, as with `left_out` for exports.

**Two ways to apply the rule**.

1. In place: `doc.objects_for(Purpose::Calculate(CalcKind::Schedule))` in the loop. Selection, snapping, drawing, small queries and every report loop use it.
2. As a typed copy for the pipelines that write files. A newtype `ExportDoc<'a>` has one builder, `leftout::prepare_for(doc, opts)`. The public exporters take `&ExportDoc`. A caller that skips the copy gets a compile error. This closes the gap of section 2.4 (D12).

`Document::official()` returns `Cow<Document>` and is free for a document without proposals. It is the adapter for deep call trees such as the terrain index. It clones for a document with proposals, so a session caches it by revision.

**The export copy**. `leftout::prepare` grows one step. It drops objects with a proposal unless `include_proposals` is set. `LeftOut` gets a `proposals` count and a note line. `export.file` answers `left_out.proposals`. An export with proposals marks them: a property set in IFC, a layer prefix in DXF, `extras` in glTF (D11).

**Caches**. Every cache keyed by the editor revision gets a second key: the purpose class (official, with proposals, and the shown groups). Section 5 lists the caches.

### 4.3 Moving the old accessors out of reach

The field `objects` stays public until the last stage. Four tools close the gap in order.

1. **Scan test with an allow list**. The test `crates/kadakas-core/tests/objects_gate.rs` walks `crates/*/src` and drops test code. It finds `.objects` followed by an iterator call or a `for`. Each hit has a line `path<TAB>function<TAB>purpose` in `objects_gate_allow.txt`. The key is the function name, so edits do not break it.
2. **Ratchet**. An entry that matches no hit fails the test, as in `check_number_format.py`. A new hit fails the test. The list only shrinks. Stage 0 fills it with the 247 functions of this survey.
3. **Use of `Everything`**. The calls `objects_for(Purpose::Everything)` and `all_objects()` appear only in the files of a second list. This stops `Everything` from becoming the lazy answer.
4. **Compile-time end state**. The allow list ends with engine entries only. Then `Document::objects` becomes `pub(crate)` and later private. Other crates use `objects_for`, `all_objects`, `object(id)` and `objects_len()`. Tests in other crates use `all_objects()`.

`#[deprecated]` on the field is the wrong tool. It warns on about 1000 test lines at once, and CI treats warnings as errors. The scan test is quieter and gives a list.

### 4.4 (c) Proposal groups and alternative sets

- A **proposal group** is a document resource. The new `ResourceBody` variant is `ProposalGroup { name, color, author, origin, time, note, set, order }`.
- An **alternative set** is a resource too: `ProposalSet { name, question, order }`. A group points to its set with the field `set`. The set holds no list of options.
- The back pointer avoids a conflict. Two people who add an option together each create a new group. A list inside the set is one property, and one of the two edits loses.
- Resources already sync (`Kind::Resource`, property `body`). They are part of versions, count their users and refuse a delete in use (`ResourceInUse`). Groups and sets reuse all of it. An old build keeps an unknown resource type as `ResourceBody::Unknown` and saves it again.
- The object points to its group with `Proposal.group`. `library::usage` and the purge count that reference.
- Library import (DOC-16) and the resource list skip the two types. They are not reusable definitions.
- The option that shows per set is not in the document. It is a personal view setting of the session. Saved views (`SavedView`, `sheets.rs:196`) get a field `proposal_options`. It maps a set to a group or to none. That field is drawing data and is shared.
- Accepting an option of a set is one command. It accepts the group and rejects its siblings into the Rejected list. It is one undo step.
- Limits (options per set, sets per document, proposals per document) come from a benchmark. The owner chooses them (D13).

The alternative to resources is a document field like `issues`, with its own `Kind` in `kadakas-props`. It costs new kinds in `op.rs`, `docsync.rs`, `validate.rs` and `materialize.rs`. D9 records the choice.

**Accept and reject, as the filter sees them**.

- Accept of a new object: `unset proposal`. One undo step per call, also for a whole group.
- Accept of a proposed change (`replaces` is set): copy shape, attrs and data from the proposal into the original, then delete the proposal. The original keeps its id. So tags, dimensions, issue anchors and history stay valid (D5).
- Reject: set the status `rejected` and list the object in Rejected. Nothing is deleted until the person deletes it (D10).

## 5. Risks

1. **Caches keyed by revision**. A cache that returns the result of an earlier listing leaks proposals into official numbers. Each of these caches gets the purpose class in its key:
   - `SiteCache` (`kadakas-site/src/model.rs:271`).
   - The terrain index (`kadakas-site/src/terrain.rs:41`).
   - `MeshCache` in `Session::scene` (`kadakas-api/src/lib.rs:1395`).
   - The section cache (`kadakas-api/src/x3d_api.rs:343`).
   - `ExpansionCache` (`kadakas-core/src/component.rs:335`).
   - The plan cache in `kadakas-app/src/surface_draw.rs`.
   The property test of stage 3 covers all of them.
2. **Site, terrain and texture beds**. Five functions feed cut and fill, contours, slopes and the ground height of plants:
   - `modifiers_for` (`kadakas-site/src/model.rs:80`).
   - `Index::new` (`kadakas-site/src/terrain.rs:92`).
   - `host` (`kadakas-site/src/grading/mod.rs:142`).
   - `beds_for` (`kadakas-core/src/texture_bed.rs:222`).
   - `scene_ext` (`kadakas-site/src/draw.rs:242`).

   A proposal pad or bed changes the picture and never the official volume. One function serves both. This is the largest change (L).
3. **Export entry points beside `export_impl`**. They are the SVG menu command, the web SVG download and the public exporter functions. The `ExportDoc` newtype is the fix. Until then a gate test lists every caller of `kadakas_io::export_file`, `kadakas_ifc::export_*`, `kadakas_x3d::export_*` and `kadakas_print::export_pdf`.
4. **Agent defaults and client-side totals**. A script sums areas from `objects.list`. The two treat proposals in different ways, so the numbers differ from `report.areas`. `document.summary` (`lib.rs:1677`) is the count that an agent reads first. The default of every list is "exclude". The answer says how many it left out.
5. **References between objects**. A group with official and proposal members is one case. A tag, dimension or callout anchored to a proposal is another. A component instance whose symbol only a proposal uses is a third. The accept of a change that replaces an original is the fourth. `dependants` (`command/steps.rs:781`) and `annot_after` (`command/annot_ops.rs:82`) already follow such links. The design adds three rules. Every member of a proposal group is a proposal (checked in `Document::validate`). An annotation made on a proposal joins its group. The command layer refuses to parent a proposal under an official group.
6. **Copies carry identity**. "Propose a change to this object" copies `data`. The copy then holds a provenance tag (`kadakas.prov`). Update from DXF matches that tag (`kadakas-io/src/dxf/update.rs:683`). The copy holds site names (`SITE_KEY`) and a plug-in instance id (`plugin_api.rs:245`) too. Worksheet table links come along as well. The copy drops or rewrites these keys. Update from DXF lists only official objects (the rows marked AX).
7. **Performance of the filter**. The filter is one pointer test per object. The hot loops are the hit test (every object, once per mouse move), the scene build and the snap geometry. A micro-benchmark on a large model gives the number (stage 0). A cached proposal count that skips the filter is not part of the design. A stale count leaks proposals.
8. **A missed call site**. Before the last stage the scan test is the only guard. A loop that the regex misses is possible, for example an alias like `let m = &doc.objects`. The property test of stage 2 catches it by output. The private field of stage 5 removes the whole class of error.
9. **Undo and redo**. The change set holds full object states before and after (`ObjChange`). So a state change undoes by itself. Per-user collab undo turns `unset proposal` back into `set proposal`. The two-object accept (copy into the original, delete the proposal) is one step. The test: accept, undo, and every official total returns.
10. **Version diff colours**. A new proposal is a new object. Without a rule it shows green, as added. The overlay needs the status `Proposed`: a fourth colour with a pattern and a mark, because colour is never the only sign. An accept shows as added, because the object enters the official model. D8 records the default of `version.diff`.
11. **Old files and old builds**. Old files have no field and load as they are. New files in old builds are D1.
12. **Sheets**. `Viewport::shows` has no object filter. A viewport made before proposals shows them, and a PDF prints them. The rule goes into `shows` with a viewport option `proposals`, off by default. Page composition (`print/src/compose.rs:122`) and the 3D viewport document (`compose.rs:280`) follow.
13. **Rejected proposals grow the file**. They stay until the person deletes them. A limit warns and never deletes (consent rule).

## 6. Decisions for the owner

| # | Decision | Recommendation. |
|---|---|---|
| D1 | Old builds and files with proposals: refuse (format 4 only with proposals), or open and show them as official. | Refuse with the existing "newer Kadakas" error. Refuse old clients on a shared document. |
| D2 | Storage: typed field `proposal`, or key `kadakas.proposal` in `data`. | Typed field. |
| D3 | Sync: one property `proposal`, or `proposal.status` and `proposal.note`. | One property. Notes live in the issue thread. |
| D4 | Stored statuses. | `open`, `needs_work`, `rejected`. Accepted means the field is gone. Unknown statuses fail closed. |
| D5 | Accept of a change: merge into the original, or replace it with the copy. | Merge into the original. Ids, tags and anchors stay valid. |
| D6 | Default of agent lists (`objects.list`, `objects.query`, `mesh.*`, `document.summary`). | Exclude. The answer has `proposals_hidden`. `include_proposals` opts in. |
| D7 | Selection and snapping. | Snap is off by default. Select takes proposals in Proposal mode or from the Proposals palette. |
| D8 | `version.diff` and the compare overlay. | Official changes as today. Proposals as a fourth kind `proposed`. Accepted objects show as added. |
| D9 | Home of groups and sets: resources, or a document field like `issues`. | Resources. Library import and purge skip them. |
| D10 | Rejected proposals. | Keep them until the person deletes them. A limit only warns. |
| D11 | Export with proposals. | Mark them in the file: an IFC property set, a DXF layer prefix, glTF extras. |
| D12 | Compile-time gate for exports: the `ExportDoc` newtype. | Yes, in stage 1. |
| D13 | Limits (proposals per document, sets, options per set) and the gate form: a Rust test, or a Python script like the number gate. | Measure first and choose with the owner. A Rust test with an allow list. |

Two smaller rules need the owner as well. Published revisions (PLAT-30) never take proposals. An annotation made on a proposal joins its group.

## 6a. Decisions taken

On 2026-11-02 the owner accepted the recommendation of every decision, D1 to D13, in the table above. The two smaller rules also hold. Published revisions (PLAT-30) never take proposals. An annotation made on a proposal joins its group.

Two points stay open on purpose. The limits of D13 (proposals per document, sets, options per set) come from a measurement. The owner chooses the numbers after that. The look of a proposal and the command layer of the propose mode are designed in their own steps.

## 7. Plan

Each stage merges alone and keeps every test green. No proposal exists before the Proposal mode ships. So stages 0 to 5 change no visible behavior. Property tests create proposals through a test helper to prove the filter early.

| Stage | Size | Content. |
|---|---|---|
| 0 | M | The field, `Proposal`, `Purpose`, `Rule`, `objects_for`, `all_objects`. The scan test with its allow list of 247 entries. The sync arm, the validation, the format 4 rule, the collaboration doc and the filter benchmark. |
| 1 | M | Exporters. `leftout::prepare` drops proposals. `LeftOut.proposals`. `include_proposals` on `export.file`. The `ExportDoc` newtype. The SVG menu and web paths. PLAT-30 publish. |
| 2 | L | Calculations. The rows marked AX and XO: reports, schedules, irrigation, worksheet database, checks, tags, update from DXF, agent lists and `document.summary`. |
| 3 | L | Site, terrain, texture beds, sections and the cache keys. These are the BP rows. |
| 4 | M | Selection, snapping, drawing and the viewport rule. These are the IN rows and `Viewport::shows`. The switcher state `Shown` arrives here. |
| 5 | M | Close the gate. Engine sites move to `all_objects()`. The field becomes private. Tests in other crates move to the accessors. |

After stage 5 the feature rows of REV-12 follow in the order of the row: the mode and the look, accept and reject, the agent propose mode (AGR-07), then groups, sets and the switcher.

### Tests per stage

**Stage 0**

- `objects_gate.rs`: the scan finds no hit outside the allow list and no stale entry.
- Rule table: a test lists all variants of `Purpose` and reads the rule of each.
- Round trip: a document with proposals loads and saves with the same JSON. A test with the old struct shape (like `forward_compat.rs`) keeps the key in `extra`. Unknown subfields and unknown statuses survive.
- Format: a file without proposals stays at version 3. A file with one is version 4.
- Sync: random edits that set and unset `proposal` converge. The generators come from the existing `kadakas-props` property tests.
- Benchmark: `objects_for(Official)` against `values()` on a large document, with the tool of `docs/doc-load-benchmark.md`.

**Stage 1**

- Property: for random documents, every export of `A` and of `A + proposals` is byte identical. The report of the second has `left_out.proposals = n`.
- With `include_proposals: true` the export holds the proposals and marks them.
- A test lists every caller of the public exporters. It fails on a call without an `ExportDoc`.

**Stage 2**

- The main property test: a proposal never changes any official total. It builds `A`, copies it as `B` and adds random proposals to `B`. Then it compares a battery of results. All results are equal. The battery holds:
  - The plant schedule, the plant list and the area reports.
  - The massing report and the worksheet database rows.
  - The irrigation analysis, `site.check` and cut and fill.
  - `document.summary`, `objects.list` and `objects.query`.
  - The snap targets and the SVG of the official view.
- With `include_proposals` the totals of `B` are a superset.
- Metamorphic test: accept all proposals in `B`. The battery of `B` then equals the battery of `A` with the same objects added.
- Schema test: every API method that lists objects has `include_proposals` in its schema. A list of listing methods in `describe` drives the test.

**Stage 3**

- The property test of stage 2 runs on documents with proposal grading modifiers, texture beds and site models.
- Cache test: ask for the official and the drawn result in turn at one revision. Neither changes.

**Stage 4**

- Hit test, box select and lasso follow the view state for proposals.
- Snap ignores proposals by default. `snap.set {proposals: true}` turns them on.
- A viewport and a PDF page leave proposals out by default.
- The scene with and without proposals has identical official triangles.

**Stage 5**

- The workspace compiles with a private `objects`. The allow list holds engine entries only. A negative test with a forbidden loop shows that the gate fails.

## 8. Not covered here

- The look of a proposal: tint, dashed outline, badge and ghosting. It is the second step of the row.
- The command layer of the propose mode (AGR-07): how an agent edit of an official object becomes a proposal copy.
- The server side of the protocol check (PLAT-09, in `kadakas-server`).
