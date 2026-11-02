# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Agent API gaps from a furniture model (AGR-09). `objects.delete {ids}` is a listed method (MCP, Python, scripts) with the checks of `delete_objects` and one undo step. The answer to an unknown method names listed methods. `selection.get` and `selection.set` have the shape `{ids, faces, edges}` for the selected faces and edges of surface models. The 3D modeler mirrors its picks into the session, but no host connects a face pick to an agent yet. `view.get` gives the 3D camera in the shape that `render.png`, `mesh.render` and `view.camera` take. These calls refuse an `eye` that disagrees with `yaw`, `pitch` and `distance`.
- The agent sees what the person points at (AGR-10). `view.get` has the field `pointer`: `{over_canvas, canvas [x, y], age_ms, navigating, focused, mode}`. In the plan it has the drawing point and the object under the pointer (`point`, `ids`). In 3D it has the ray under the pointer (`ray {origin, direction}`, accepted by `mesh.pick`). It also has `hit`, the answer of `mesh.pick` for that ray. `hit` has `edge` when the pointer is on an edge. A component, a slab or a plant can be nearer than every surface model. Then `hit` is `null` and `nearest_object` names that object. The field is `null` when the pointer is not over the drawing area. Only the app delivers it, only a call of the agent reads it, and it is not saved or shared.
- Preferences has "Agents may see my pointer" (on by default). Off: `pointer` is `null` and `pointer_reason` is `disabled_by_user`. An eye in the status bar shows for 10 seconds after an agent read the pointer. The Agent Activity panel lists the reads, at most one line a minute. A client gets 60 pointer answers a second at most.

### Changed

- Release files have one name pattern, `Kadakas-<version>-<os>-<arch>.<ext>`, with the exact version in every name. The notes of a release start with a table that tells which file to download.
- The macOS app bundles have a valid ad-hoc seal. A browser download no longer says "Kadakas.app is damaged". macOS 15 says "Apple could not verify Kadakas is free of malware" (macOS 14 and older: "cannot be opened because the developer cannot be verified"). The release notes tell how to open the app.

### Fixed

- The X and Y numbers in the status bar no longer stay behind. They show nothing when the pointer is outside the drawing area. In 3D they show the point where the pointer meets a surface model. They show nothing while the view moves or when no model is under the pointer. Before, they kept the last place of the plan.
- `mesh.near` and `mesh.pick` no longer find a hidden surface model. This covers Hide, a phase display rule, a sheet layer and a symbol that is being edited. They used only the visibility of the layer and the class before.
- The map readout in the status bar keeps its place when the pointer leaves the drawing. The bar does not jump. The X and Y box is wide enough for large coordinates.

## [0.1.0-alpha.1] - 2026-10-27

First public build. Kadakas is a CAD program for landscape architects. It is not stable yet.

### Added

- Desktop app for 2D landscape design: layers, classes, shapes, curved paths, hatches, dimensions, symbols, plants, irrigation, site models with grading, contours and cut and fill volumes, sheets and PDF print.
- 3D modeler with Push/Pull, Follow Me, Offset, solid tools, scenes and analytic curves.
- Circles, arcs and polygons in the 3D modeler keep their exact radius, center and angles (MOD-15). Entity Info shows them and sets the number of segments in one undo step, the center is a snap point, and the API has `mesh.curve_info` and `mesh.set_segments`. Files with this data are saved as surface model version 2. Older versions of the program open such a model read only.
- Solid tools offer a snap for near-touches (MOD-19). A vertex 0.0025 to 0.01 mm off a face of the other solid is refused as before. With consent it moves onto the face in the same undo step. Consent is `snap_near` in the API or the button "Snap and retry" in the modeler. The result lists each moved vertex. `SolidError` is no longer `Eq`, because the new variants hold distances in `f64`.
- Import and export of DXF, IFC (including ifcZIP), STEP, OBJ, STL and glTF. DWG works through a converter that you install.
- The `.kad` file: a ZIP container that any ZIP tool can open. A plain JSON form is available for git and diffs.
- Agent API over JSON-RPC and MCP (`kadakas mcp`). Every change to a drawing is an undoable command, and a person can review what an agent did.
- Python client (`import kadakas`), JavaScript scripts and sandboxed WebAssembly plug-ins.
- Launcher that starts the app and fetches signed updates. You can choose the stable or the beta channel.
- Welcome dialog on the first start: how you want to work, language, units and number format, and under More options theme, mouse or trackpad, crash reports and updates. Help > Show welcome opens it again.
- Number format setting (decimal point or comma, digit grouping) for every number field in the app and the modeler.
- Crash reports and a feedback button. Nothing is sent without your consent.
- Collaboration client for a shared drawing.
- English and Estonian user interface.
- Installers for macOS, Windows and Linux.
- Smoke test of the installers: every release package is installed on a clean runner, started, and checked before the release is made.

### Known limitations

- The installers are not signed. Your system warns you at the first start.
- DWG needs a converter that you install separately.
- The 3D modeler has no exact solids yet.

[Unreleased]: https://github.com/nemecec/kadakas-app/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/nemecec/kadakas-app/releases/tag/v0.1.0-alpha.1
