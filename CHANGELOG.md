# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/nemecec/kadakas-app/commits/main
