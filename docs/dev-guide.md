# Cadektor developer guide

This guide gives the rules for all work on Cadektor, by people and by AI agents.

## Product rules

- Cadektor is a clean-room CAD application for landscape architects. Its workflow and terms follow Vectorworks 2026, so Vectorworks users feel at home.
- Use only public information: help pages at app-help.vectorworks.net, product pages and the published shortcut list. Do not use the Vectorworks software itself.
- Write all text and code in your own words. Do not copy help text, icons or graphics. Do not use Vectorworks trademarks as names in the product.
- Before you design a feature, read `docs/research/vectorworks-public-features.md` and the related public help page. Match the tool modes, the parameters, the terms and the default keys.

## Architecture

The "Depends on" column lists the other Cadektor crates that a crate uses (`cargo tree`, normal dependencies). Dependencies marked "native" apply only when the target is not `wasm32`.

| Crate | Content | Depends on |
|---|---|---|
| `cadektor-core` | Document model, UUID ids (`uid`), commands, undo, the change-event log (`change`), geometry (`geom`: shared polygon and segment helpers), the ground trait (`ground`), caches (`cache`: `RevCache`, `LruCache`), curves, 2D editing (`edit2d`: transforms, arrays, offset, trim, fillet, booleans), snapping math, visibility modes, command schema, mesh, file format and its versions | none of ours |
| `cadektor-sys` | Platform helpers without document data: atomic and private file writes, process groups and pid checks, config, state, runtime and cache folders, private temp folders, the agent runtime file (native only) | none of ours |
| `cadektor-crs` | Coordinate systems: EPSG lookup, transformers, axis order (proj4rs; wasm-compatible) | core |
| `cadektor-plants` | Plant placement algorithms, the plant catalog and its CSV, plant schedules and the plant list worksheet (pure, wasm-compatible) | core |
| `cadektor-render` | Camera, standard views, picking, CPU rasterizer | core |
| `cadektor-sheet` | Worksheets: spreadsheet grid, formulas, database rows over objects (pure, wasm-compatible) | core, plants |
| `cadektor-annot` | Data tags, legends and annotation output on top of the worksheet code (pure, wasm-compatible) | core, sheet |
| `cadektor-site` | Site model: TIN, contours, slope, site modifiers, cut and fill, grading (pure, wasm-compatible) | core |
| `cadektor-irrigation` | Irrigation catalog, hydraulics, network solver, layout helpers, reports (pure, wasm-compatible) | core |
| `cadektor-x3d` | glTF, GLB and OBJ writers, section and elevation drawings from the 3D scene (pure, wasm-compatible) | core, render |
| `cadektor-script` | Python discovery, limited process runs, plug-in object definitions and the reset protocol, the script host (native only; no Python is embedded) | core, sys |
| `cadektor-io` | DXF, Shapefile and GeoJSON import and export, DWG through an external converter, CRS conversion through `crs` (native only) | core, annot, sys, crs, site |
| `cadektor-gis` | WMS, WMTS and WFS services, geoimages, tile cache, document CRS change, coordinate readout (native only; has the HTTP client) | core, crs, io |
| `cadektor-ifc` | IFC4 export, IFC2x3/IFC4/IFC4x3 import, STEP writer and streaming reader (native only) | core, crs, sys, io, site |
| `cadektor-print` | Sheet composition and output: PDF, SVG and PNG pages from sheet layers | core, sys, render, sheet, annot, site, x3d |
| `cadektor-api` | JSON-RPC session and methods for agents (`Session`, `METHODS`, `describe`) | core, render, sheet, annot, site, irrigation, plants, crs; native only: sys, io, ifc, gis, print, script, x3d. Also: `SessionLimits`, the event hub (`events`) |
| `cadektor-cli` | The `cadektor` program: `serve`, `mcp` (domain tools, `--mcp-flat`), `render`, `import`, `export` | core, sys, api |
| `cadektor-app` | egui and wgpu desktop app and web build | core, render, sheet, annot, site, irrigation, plants, print, api; native only: sys, gis, script |

The layers, from the bottom (a crate uses only crates above it; `cargo tree` is the truth):

```
core            sys
  |              |
  +-- render  site  sheet  irrigation  crs  plants       (each uses core; sheet also plants)
  |     |       |     |
  |   x3d     annot --+                                   (x3d: core, render; annot: core, sheet)
  |     |       |
  |     +- print -+-- sys                                 (print: also sheet, site)
  |
  io (annot, site, crs, sys)  ->  gis (crs, io),  ifc (crs, io, site, sys)
  script (sys)
  api  = everything above;  cli = core, sys, api;  app = api and the UI crates
```

Nothing below `api` depends on `script` any more: `io` runs its converter through `sys`.

- Put each feature in its own crate or module, for example `cadektor-io` for file formats and `cadektor-site` for terrain. Keep the interface small.
- Core, crs, plants, render, site, annot, x3d, api and app must build for `wasm32-unknown-unknown` (the wasm check covers them). `crs` uses `proj4rs` without its default features on wasm, because the `aeqd` feature compiles C code. Keep file access, threads and GPU code out of the pure crates; put native-only code behind `#[cfg(not(target_arch = "wasm32"))]`.
- All document changes go through `Command` values. The GUI, the API and scripts use the same commands, so every change can be undone.
- When you add a `Shape` variant or an API method, keep the edits to shared files small and in one place.
- Agents must reach every document feature through the API. Add an example to `describe`.
- After you add or change a `Command`, renew its JSON Schema: `UPDATE_SCHEMA=1 cargo test -p cadektor-core --test command_schema`, and commit `crates/cadektor-core/tests/command_schema.json`. The schema comes from `schemars`; the types inside a command that other crates own (shapes, resource bodies, patches) are described in `crates/cadektor-core/src/schema.rs`.
- After you add or change an API method, renew the Python package: `CADEKTOR_BIN=<your binary> python python/tools/codegen.py`. The test `python/tests/test_drift.py` fails when `python/cadektor/_api.py` and `describe` differ.

## Many documents in one process

A host can run many `Session` values in one process (PLAT-12). The rules:

- A session owns all of its state. A library crate has no `static` with document data. A `static` is allowed for an id allocator (the editor revision, the GUID counter) or for a table that never changes.
- Every cache is per session and bounded: `RevCache` for one entry by revision, `LruCache` for many. The site analysis cache is in `SiteCache`, which `Session` owns.
- `SessionLimits` sets the limits of one session: objects, memory, undo history bytes, request time, picture size and so on. `Session::set_limits` changes them. A call that changes the document and breaks the object or memory limit is undone. A session that is over the limit without a change only drops its caches, so reads and saves keep working. `document.load` and `file.open` refuse a document that does not fit before they replace the open one. The estimate counts the objects, the undo steps and the change log.
- New ids are UUIDv7 (`Document::alloc_id`). A web build must call `cadektor_core::uid::set_host_seed(ms, salt)` once before it makes a document (the app does this in `new_web` with `Date.now()` and `crypto.getRandomValues`), because `std` has no clock or process there. Tests and fixtures use `Document::with_sequential_ids()` or `cadektor serve --sequential-ids`.

## Quality rules

- Write unit tests for all logic. Add property tests (proptest) where random input can find bugs.
- Add Python behavior tests in `tests/behavior` for features that agents can reach through the API.
- Keep line coverage of each library crate at 95% or more (`cargo llvm-cov -p <crate> --summary-only`). CI enforces it with `scripts/coverage.sh` (all crates, or name some: `scripts/coverage.sh print api`). The `cadektor-cli` crate has a bar of 90% (its tests run the binary). The app crate (egui and wgpu windows) is left out.
- Dependencies must have a permissive license. CI runs `cargo deny check` (settings in `deny.toml`). After you add or update a dependency, run `scripts/notices.sh` and commit `THIRD_PARTY_NOTICES.txt`; the installers ship it (`packager.toml`, `resources`).
- Test helpers must run on Windows too: use `target/debug/cadektor.exe` there, and read pipes with a thread and a queue, not with `select`.
- Reject numbers that are not finite. Saved files must always open again.
- Plain English in documents and comments: short sentences, active voice.

## Checks before a commit

```
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p cadektor-core -p cadektor-crs -p cadektor-plants -p cadektor-render -p cadektor-api -p cadektor-site -p cadektor-annot -p cadektor-x3d -p cadektor-app --target wasm32-unknown-unknown
cargo build -p cadektor-cli && uv run --with pytest --with ezdxf --with ifcopenshell pytest tests/behavior python/tests -q
uv run --no-project --with pytest pytest python/tests -q   # the Python package (set CADEKTOR_BIN as for the behavior tests). Both folders also run in one command: pytest tests/behavior python/tests
```

All five must pass. The wasm check covers the app crate too: keep native-only code (files, threads, sockets, Python) behind `#[cfg(not(target_arch = "wasm32"))]`. The Python package tests belong to check 5. End each commit message with the co-author line that the project uses.

## Parallel work in worktrees

- Use your own build folder: `CARGO_TARGET_DIR=<repo main folder>/target-<feature>`. Shared build folders mix binaries from different branches.
- Disk space is limited. Set `CARGO_INCREMENTAL=0` and `CARGO_PROFILE_DEV_DEBUG=0`. Run release and coverage builds only when you need them.
- Point the behavior tests at your binary: `CADEKTOR_BIN=$CARGO_TARGET_DIR/debug/cadektor`.
- Other branches add `Shape` variants, commands and API methods at the same time. Add catch-all arms where a match does not need every kind, and keep shared edits small.
- Before you finish, merge `main` into your branch, resolve conflicts, and run the checks again.
