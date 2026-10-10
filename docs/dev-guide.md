# Kadakas developer guide

This guide gives the rules for all work on Kadakas, by people and by AI agents.

## Product rules

- Kadakas is a clean-room CAD application for landscape architects. Its workflow and terms follow Vectorworks 2026, so Vectorworks users feel at home.
- Use only public information: help pages at app-help.vectorworks.net, product pages and the published shortcut list. Do not use the Vectorworks software itself.
- Write all text and code in your own words. Do not copy help text, icons or graphics. Do not use Vectorworks trademarks as names in the product.
- Before you design a feature, read `docs/research/vectorworks-public-features.md` and the related public help page. Match the tool modes, the parameters, the terms and the default keys.

## Architecture

The "Depends on" column lists the other Kadakas crates that a crate uses (`cargo tree`, normal dependencies). Dependencies marked "native" apply only when the target is not `wasm32`.

| Crate | Content | Depends on |
|---|---|---|
| `kadakas-core` | Document model, UUID ids (`uid`), commands, undo, the change-event log (`change`), geometry (`geom`: shared polygon and segment helpers), the ground trait (`ground`), caches (`cache`: `RevCache`, `LruCache`), curves, 2D editing (`edit2d`: transforms, arrays, offset, trim, fillet, booleans), snapping math, visibility modes, command schema, mesh, file format and its versions | none of ours |
| `kadakas-sys` | Platform helpers without document data: atomic and private file writes, process groups and pid checks, config, state, runtime and cache folders, private temp folders, the agent runtime file (native only) | none of ours |
| `kadakas-crs` | Coordinate systems: EPSG lookup, transformers, axis order (proj4rs; wasm-compatible) | core |
| `kadakas-plants` | Plant placement algorithms, the plant catalog and its CSV, plant schedules and the plant list worksheet (pure, wasm-compatible) | core |
| `kadakas-render` | Camera, standard views, picking, CPU rasterizer | core |
| `kadakas-sheet` | Worksheets: spreadsheet grid, formulas, database rows over objects (pure, wasm-compatible) | core, plants |
| `kadakas-annot` | Data tags, legends and annotation output on top of the worksheet code (pure, wasm-compatible) | core, sheet |
| `kadakas-site` | Site model: TIN, contours, slope, site modifiers, cut and fill, grading (pure, wasm-compatible) | core |
| `kadakas-irrigation` | Irrigation catalog, hydraulics, network solver, layout helpers, reports (pure, wasm-compatible) | core |
| `kadakas-x3d` | glTF, GLB and OBJ writers, section and elevation drawings from the 3D scene (pure, wasm-compatible) | core, render |
| `kadakas-script` | Python discovery, limited process runs, plug-in object definitions and the reset protocol, the script host (native only; no Python is embedded) | core, sys |
| `kadakas-plugin-abi` | The versioned ABI of WebAssembly plug-ins: manifest, parameter schema, host and guest messages, helpers for plug-in authors (pure, wasm-compatible; no Kadakas dependency, so a plug-in stays small) | none of ours |
| `kadakas-plugin` | The plug-in host: a runtime-neutral engine interface, the wasmtime engine with fuel and a memory cap and no imports (native only), a stub engine for the browser build, trust by module hash, the registry, checks of the answer. It compiles for wasm32 but runs plug-ins only on native targets | core, plugin-abi; native only: sys |
| `kadakas-update` | Launcher logic: signed update manifests (ed25519), the HTTP trait, checked downloads, safe unpacking, versioned installs with rollback, launcher settings, UI flavors (native only; holds no private key) | sys |
| `kadakas-io` | DXF, Shapefile and GeoJSON import and export, DWG through an external converter, CRS conversion through `crs` (native only) | core, annot, sys, crs, site |
| `kadakas-gis` | WMS, WMTS and WFS services, geoimages, tile cache, document CRS change, coordinate readout (native only; has the HTTP client) | core, crs, io |
| `kadakas-ifc` | IFC4 export, IFC2x3/IFC4/IFC4x3 import, STEP writer and streaming reader (native only) | core, crs, sys, io, site |
| `kadakas-print` | Sheet composition and output: PDF, SVG and PNG pages from sheet layers | core, sys, render, sheet, annot, site, x3d |
| `kadakas-api` | JSON-RPC session and methods for agents (`Session`, `METHODS`, `describe`) | core, render, sheet, annot, site, irrigation, plants, crs; native only: sys, io, ifc, gis, print, script, plugin, plugin-abi, x3d. Also: `SessionLimits`, the event hub (`events`) |
| `kadakas-cli` | The `kadakas` program: `serve`, `mcp` (domain tools, `--mcp-flat`), `render`, `import`, `export` | core, sys, api |
| `kadakas-launcher` | The launcher program (PLAT-06): an egui window that shows versions, channel, flavor and profile, and has Launch, Check, Update and Roll back. The window code only draws; the state is in `model.rs`. Left out of the coverage bar like the app (native only) | sys, update |
| `kadakas-sign-manifest` (in `scripts/sign-manifest`) | The CI tool `sign-manifest`: `keygen`, `public`, `build`, `sign`, `verify` for update manifests. Holds the signing code (feature `sign` of `kadakas-update`); the launcher does not | sys, update (feature `sign`) |
| `kadakas-ui-core` | What every UI program needs, without egui (PLAT-03): the tool state machines and tool settings, picking and box selection, the snapping glue and smart-point key, view state (pan, zoom, screen and model transforms) and view history, the Data bar entry logic, keys and the keymap (action ids, default keys, JSON format for profiles). Input is abstracted (`keys::Key`, `Modifiers`, `tools::Input` with model coordinates). Pure and wasm-compatible | core, annot, irrigation, plants, render, site |
| `kadakas-app` | egui and wgpu desktop app and web build. A thin adapter: it turns egui input into ui-core events and draws the ui-core previews | core, ui-core, render, sheet, annot, site, irrigation, plants, print, api; native only: sys, gis, script |

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
  script (sys)    plugin-abi  ->  plugin (core, plugin-abi; sys on native)
  ui-core (core, annot, irrigation, plants, render, site)  ->  app (and any other UI program)
  api  = everything above;  cli = core, sys, api;  app = api and the UI crates
```

Plug-ins (`docs/plugins.md`) are WebAssembly modules. The example plug-in lives in `plugins/raised-planter`, outside the workspace (it builds for `wasm32-unknown-unknown` with its own lock file). The tests use its committed module `crates/kadakas-plugin/tests/fixtures/raised_planter.wasm`; run `scripts/build-plugin-fixtures.sh` after you change the example or the ABI, and commit the new module.

Nothing below `api` depends on `script` any more: `io` runs its converter through `sys`.

- `kadakas-update`, `kadakas-launcher` and `kadakas-sign-manifest` are native only (files, sockets, processes, windows). They are not in the wasm check. Leave them out of the `cargo check ... --target wasm32-unknown-unknown` line, and do not make a wasm crate depend on them.
- The launcher holds no secret. It has public keys only (`crates/kadakas-launcher/trusted-keys.txt`). Private keys exist in CI secrets only. Signing code lives behind the `sign` feature of `kadakas-update`, and only `sign-manifest` and tests turn it on.
- UI logic that has no egui code (tools, picking, snapping glue, view state, keymap) belongs in `kadakas-ui-core`, so another UI program can reuse it. The app only converts egui input into ui-core events and draws. Do not add `egui` to ui-core.
- Put each feature in its own crate or module, for example `kadakas-io` for file formats and `kadakas-site` for terrain. Keep the interface small.
- Core, crs, plants, render, site, annot, x3d, ui-core, plugin-abi, plugin, api and app must build for `wasm32-unknown-unknown` (the wasm check covers them). `crs` uses `proj4rs` without its default features on wasm, because the `aeqd` feature compiles C code. Keep file access, threads and GPU code out of the pure crates; put native-only code behind `#[cfg(not(target_arch = "wasm32"))]`.
- All document changes go through `Command` values. The GUI, the API and scripts use the same commands, so every change can be undone.
- When you add a `Shape` variant or an API method, keep the edits to shared files small and in one place.
- Agents must reach every document feature through the API. Add an example to `describe`.
- After you add or change a `Command`, renew its JSON Schema: `UPDATE_SCHEMA=1 cargo test -p kadakas-core --test command_schema`, and commit `crates/kadakas-core/tests/command_schema.json`. The schema comes from `schemars`; the types inside a command that other crates own (shapes, resource bodies, patches) are described in `crates/kadakas-core/src/schema.rs`.
- After you add or change an API method, renew the Python package: `KADAKAS_BIN=<your binary> python python/tools/codegen.py`. The test `python/tests/test_drift.py` fails when `python/kadakas/_api.py` and `describe` differ.

## Many documents in one process

A host can run many `Session` values in one process (PLAT-12). The rules:

- A session owns all of its state. A library crate has no `static` with document data. A `static` is allowed for an id allocator (the editor revision, the GUID counter) or for a table that never changes.
- Every cache is per session and bounded: `RevCache` for one entry by revision, `LruCache` for many. The site analysis cache is in `SiteCache`, which `Session` owns.
- `SessionLimits` sets the limits of one session: objects, memory, undo history bytes, request time, picture size and so on. `Session::set_limits` changes them. A call that changes the document and breaks the object or memory limit is undone. A session that is over the limit without a change only drops its caches, so reads and saves keep working. `document.load` and `file.open` refuse a document that does not fit before they replace the open one. The estimate counts the objects, the undo steps and the change log.
- New ids are UUIDv7 (`Document::alloc_id`). A web build must call `kadakas_core::uid::set_host_seed(ms, salt)` once before it makes a document (the app does this in `new_web` with `Date.now()` and `crypto.getRandomValues`), because `std` has no clock or process there. Tests and fixtures use `Document::with_sequential_ids()` or `kadakas serve --sequential-ids`.

## Quality rules

- Write unit tests for all logic. Add property tests (proptest) where random input can find bugs.
- Add Python behavior tests in `tests/behavior` for features that agents can reach through the API.
- Keep line coverage of each library crate at 95% or more (`cargo llvm-cov -p <crate> --summary-only`). CI enforces it with `scripts/coverage.sh` (all crates, or name some: `scripts/coverage.sh print api`). The `kadakas-cli` crate has a bar of 90% (its tests run the binary). The app crate (egui and wgpu windows) is left out.
- Dependencies must have a permissive license. CI runs `cargo deny check` (settings in `deny.toml`). After you add or update a dependency, run `scripts/notices.sh` and commit `THIRD_PARTY_NOTICES.txt`; the installers ship it (`packager.toml`, `resources`).
- Attribution: every new dependency, asset (icon, font, image), dataset or cited formula or standard updates `docs/attribution.md`. Crates go in `THIRD_PARTY_NOTICES.txt` through `scripts/notices.sh`; other items go in `docs/notices-extra.txt`, which the script appends. Reviewers check this. Name a trademark only with its owner's note (see `docs/attribution.md`).
- Test helpers must run on Windows too: use `target/debug/kadakas.exe` there, and read pipes with a thread and a queue, not with `select`.
- Reject numbers that are not finite. Saved files must always open again.
- Plain English in documents and comments: short sentences, active voice.

## Checks before a commit

```
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p kadakas-core -p kadakas-crs -p kadakas-plants -p kadakas-render -p kadakas-api -p kadakas-site -p kadakas-annot -p kadakas-x3d -p kadakas-ui-core -p kadakas-plugin-abi -p kadakas-plugin -p kadakas-app --target wasm32-unknown-unknown
cargo build -p kadakas-cli && uv run --with pytest --with ezdxf --with ifcopenshell pytest tests/behavior python/tests -q
uv run --no-project --with pytest pytest python/tests -q   # the Python package (set KADAKAS_BIN as for the behavior tests). Both folders also run in one command: pytest tests/behavior python/tests
```

All five must pass. The wasm check covers the app crate too: keep native-only code (files, threads, sockets, Python) behind `#[cfg(not(target_arch = "wasm32"))]`. The Python package tests belong to check 5. End each commit message with the co-author line that the project uses.

## Parallel work in worktrees

- Use your own build folder: `CARGO_TARGET_DIR=<repo main folder>/target-<feature>`. Shared build folders mix binaries from different branches.
- Disk space is limited. Set `CARGO_INCREMENTAL=0` and `CARGO_PROFILE_DEV_DEBUG=0`. Run release and coverage builds only when you need them.
- Point the behavior tests at your binary: `KADAKAS_BIN=$CARGO_TARGET_DIR/debug/kadakas`.
- Other branches add `Shape` variants, commands and API methods at the same time. Add catch-all arms where a match does not need every kind, and keep shared edits small.
- Before you finish, merge `main` into your branch, resolve conflicts, and run the checks again.
