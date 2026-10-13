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
| `kadakas-units` | Units and formats (PLAT-05): lengths (mm, cm, m, in, ft, feet-inches with fractions), areas, volumes, angles (decimal, DMS, bearing), numbers (decimal point or comma, digit grouping), dates. Formats and parses; the current `Units` of the UI process; `Settings` for the preferences (pure, wasm-compatible) | none of ours |
| `kadakas-l10n` | Localization with Project Fluent (PLAT-05): the `.ftl` files of `locales/` built in with `include_str!`, locale negotiation with the en-US fallback, the `tr!` macro, the process language, the terminology overlay of a workspace profile (`set_overlay`, `check_overlay`; pure, wasm-compatible). Rules: `docs/localization.md` | none of ours |
| `kadakas-core` | Document model, UUID ids (`uid`), commands, undo, the change-event log (`change`), geometry (`geom`: shared polygon and segment helpers), the ground trait (`ground`), caches (`cache`: `RevCache`, `LruCache`), curves, 3D solids (`solid3d`: extrude and sweep meshes; `solid_batch`: Model > Extrude and Sweep), 2D editing (`edit2d`: transforms, arrays, offset, trim, fillet, booleans), snapping math, visibility modes, command schema, mesh, file format and its versions | units |
| `kadakas-sys` | Platform helpers without document data: atomic and private file writes, process groups and pid checks, config, data, state, runtime and cache folders, private temp folders, the agent runtime file (native only) | none of ours |
| `kadakas-feedback` | Feedback (API-47, API-48, UI-14): the report model, scrubbing of paths, tokens and e-mail addresses, dedupe by key, the capped outbox, the consent rules (`Hub::approve` is the only way out of `needs_review`), the wire payload (`docs/feedback.md`), the sender trait with retry and back-off and fakes. Native only: the folder store, the HTTP sender (`ureq`) and the sending thread. Pure parts are wasm-compatible | sys (native only) |
| `kadakas-crs` | Coordinate systems: EPSG lookup, transformers, axis order (proj4rs; wasm-compatible) | core |
| `kadakas-plants` | Plant placement algorithms, the plant catalog and its CSV, plant schedules and the plant list worksheet (pure, wasm-compatible) | core |
| `kadakas-collab` | Collaboration client (PLAT-07, PLAT-08): wire protocol, merge rules, client state machine, per-user undo, offline log, presence, transport trait. No network code and no server (the server is the separate `kadakas-server` repository). `testkit` feature: an in-memory reference sequencer for tests | core |
| `kadakas-collab-ws` | The WebSocket for the collaboration client: `WsEndpoint` and `WsTransport` (native: a blocking client with one reader thread, `ws` and `wss` with rustls and webpki roots, token in the `Authorization` header with a fall back to the `kadakas.token.<jwt>` sub-protocol; browser: a `web-sys` WebSocket behind `cfg(wasm32)`). `test-server` feature: an in-process WebSocket server on the reference sequencer, for tests | collab, core |
| `kadakas-render` | Camera, standard views, picking, CPU rasterizer, texture and picture decoding (`picture`: safe import of PNG and JPEG files with size and pixel limits, thumbnails for plant pictures) | core |
| `kadakas-sheet` | Worksheets: spreadsheet grid, formulas, database rows over objects (pure, wasm-compatible) | core, plants, units |
| `kadakas-annot` | Data tags, legends and annotation output on top of the worksheet code (pure, wasm-compatible) | core, sheet |
| `kadakas-site` | Site model: TIN, contours, slope, site modifiers, cut and fill, grading (pure, wasm-compatible) | core |
| `kadakas-irrigation` | Irrigation catalog, hydraulics, network solver, layout helpers, reports (pure, wasm-compatible) | core |
| `kadakas-x3d` | glTF, GLB and OBJ writers, section and elevation drawings from the 3D scene (pure, wasm-compatible) | core, render |
| `kadakas-script` | Python discovery, limited process runs, plug-in object definitions and the reset protocol, the script host (native only; no Python is embedded) | core, sys |
| `kadakas-plugin-abi` | The versioned ABI of WebAssembly plug-ins: manifest, parameter schema, host and guest messages, helpers for plug-in authors (pure, wasm-compatible; no Kadakas dependency, so a plug-in stays small) | none of ours |
| `kadakas-plugin` | The plug-in host: a runtime-neutral engine interface, the wasmtime engine with fuel and a memory cap and no imports (native only), the browser engine (the `WebAssembly` API of the page, wasm32 only) on a metered runtime with a module rewrite for fuel and memory, trust by module hash, stores for modules and trust (folder, browser storage, memory), the registry, checks of every answer (objects and tools). It compiles for wasm32 and runs plug-ins there too | core, plugin-abi; native only: sys |
| `kadakas-update` | Launcher logic: signed update manifests (ed25519), the HTTP trait, checked downloads, safe unpacking, versioned installs with rollback, launcher settings, UI flavors (native only; holds no private key) | sys |
| `kadakas-io` | DXF, Shapefile and GeoJSON import and export, DWG through an external converter, CRS conversion through `crs` (native only) | core, annot, sys, crs, site |
| `kadakas-gis` | WMS, WMTS and WFS services, geoimages, tile cache, document CRS change, coordinate readout (native only; has the HTTP client) | core, crs, io |
| `kadakas-ifc` | IFC4 export, IFC2x3/IFC4/IFC4x3 import, STEP writer and streaming reader (native only) | core, crs, sys, io, site |
| `kadakas-print` | Sheet composition and output: PDF, SVG and PNG pages from sheet layers | core, sys, render, sheet, annot, site, x3d |
| `kadakas-api` | JSON-RPC session and methods for agents (`Session`, `METHODS`, `describe`); `feedback_api` holds `feedback.*`, the error hints, the model excerpt and the review model | feedback, core, render, sheet, annot, site, irrigation, plants, crs, collab; plugin, plugin-abi; native only: sys, io, ifc, gis, print, script, x3d. Also: `SessionLimits`, the event hub (`events`) |
| `kadakas-cli` | The `kadakas` program: `serve`, `mcp` (domain tools, `--mcp-flat`), `render`, `import`, `export` | core, sys, api |
| `kadakas-launcher` | The launcher program (PLAT-06): an egui window that shows versions, channel, flavor and profile, and has Launch, Check, Update and Roll back. The window code only draws; the state is in `model.rs`. Left out of the coverage bar like the app (native only) | sys, update |
| `kadakas-sign-manifest` (in `scripts/sign-manifest`) | The CI tool `sign-manifest`: `keygen`, `public`, `build`, `sign`, `verify` for update manifests. Holds the signing code (feature `sign` of `kadakas-update`); the launcher does not | sys, update (feature `sign`) |
| `kadakas-ui-core` | What every UI program needs, without egui (PLAT-03): the tool state machines and tool settings, picking and box selection (the caller owns the annotation cache and passes it in `Ctx`), `effects::apply` (runs the effects of a tool in the group you are in and selects new objects), the snapping glue and smart-point key, view state (pan, zoom, screen and model transforms) and view history, the form logic of plug-in objects (`plugin_form`: fields from the parameter schema, checked input, one change per valid value) and the state of a plug-in tool run (`plugin_tool`), the Data bar entry logic, keys and the keymap (action ids, default keys, JSON format for profiles), workspace profiles (`profile`: the versioned file format, validation, merge with defaults, key conflicts, menus, tool sets, the command alias input, built-in profiles; PLAT-04, `docs/profiles.md`), the form logic of the annotation style editors (`style_edit`: the draft, checks, one `ModifyResource` command per save, follow the document, list rows; ANN-12), the logic of the collaboration UI (`collab`: the command line and environment rules, the Connect form and the browser address fragment, the state of the link and the messages for errors, the choice between the normal and the per-user undo, the colors and idle times of the other users, the sender of presence; PLAT-09, `docs/collaboration.md` section 9.2). Input is abstracted (`keys::Key`, `Modifiers`, `tools::Input` with model coordinates). Pure and wasm-compatible | core, annot, irrigation, plants, render, site, units, l10n |
| `kadakas-app` | egui and wgpu desktop app and web build. A thin adapter: it turns egui input into ui-core events and draws the ui-core previews. It gives the session the WebSocket (`collab-ws`) for `collab.connect`; the collaboration dialogs and the drawing of the other users are in `collab_ui.rs`; the Plug-ins window, the trust dialog, the form of plug-in objects and the plug-in tools are in `plugin_ui.rs` | core, ui-core, units, l10n, render, sheet, annot, site, irrigation, plants, plugin, print, api, collab-ws; native only: sys, gis, script |

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
  ui-core (core, annot, irrigation, plants, render, site, units, l10n)  ->  app (and any other UI program)
  api  = everything above (collab: core only);  cli = core, sys, api;  app = api and the UI crates
```

Plug-ins (`docs/plugins.md`) are WebAssembly modules. The example plug-ins live in `plugins/raised-planter` and `plugins/stone-row` (an object and a tool), outside the workspace (it builds for `wasm32-unknown-unknown` with its own lock file). The tests use their committed modules in `crates/kadakas-plugin/tests/fixtures/` (`raised_planter.wasm`, `stone_row.wasm`); run `scripts/build-plugin-fixtures.sh` after you change the example or the ABI, and commit the new module.

Nothing below `api` depends on `script` any more: `io` runs its converter through `sys`.

- `kadakas-update`, `kadakas-launcher` and `kadakas-sign-manifest` are native only (files, sockets, processes, windows). They are not in the wasm check. Leave them out of the `cargo check ... --target wasm32-unknown-unknown` line, and do not make a wasm crate depend on them.
- The launcher holds no secret. It has public keys only (`crates/kadakas-launcher/trusted-keys.txt`). Private keys exist in CI secrets only. Signing code lives behind the `sign` feature of `kadakas-update`, and only `sign-manifest` and tests turn it on.
- UI logic that has no egui code (tools, picking, snapping glue, view state, keymap) belongs in `kadakas-ui-core`, so another UI program can reuse it. The app only converts egui input into ui-core events and draws. Do not add `egui` to ui-core.
- Put each feature in its own crate or module, for example `kadakas-io` for file formats and `kadakas-site` for terrain. Keep the interface small.
- Units, l10n, core, crs, plants, collab, render, site, annot, x3d, ui-core, plugin-abi, plugin, feedback, api and app must build for `wasm32-unknown-unknown` (the wasm check covers them). `crs` uses `proj4rs` without its default features on wasm, because the `aeqd` feature compiles C code. Keep file access, threads and GPU code out of the pure crates; put native-only code behind `#[cfg(not(target_arch = "wasm32"))]`.
- All document changes go through `Command` values. The GUI, the API and scripts use the same commands, so every change can be undone.
- When you add a `Shape` variant or an API method, keep the edits to shared files small and in one place.
- Agents must reach every document feature through the API. Add an example to `describe`.
- Agents file feature requests and bug reports with the `feedback` tool (`docs/feedback.md`). Keep the rule: no API method, MCP tool, flag or script approves an attachment. Only `Hub::approve` does, and only the app review and `kadakas feedback review` call it. The guarantee covers agents that are limited to MCP and JSON-RPC. An agent with a shell, or with scripts or converters allowed (`--allow-agent-scripts`, `--allow-converters`), runs as the same user and can edit the outbox files: say so, and do not hide it behind the terminal check. A restricted session never writes into the config, data and feedback folders (`PathPolicy`). A method that panics must not end a session: `Session::guarded` turns it into an internal error and marks the session tainted, and `Editor::execute` takes the changes of the command back first. Tests that touch feedback use `Hub::in_memory()`, a temporary folder or `KADAKAS_FEEDBACK_DIR`, never the folder of the user, and never the network.
- After you add or change a `Command`, renew its JSON Schema: `UPDATE_SCHEMA=1 cargo test -p kadakas-core --test command_schema`, and commit `crates/kadakas-core/tests/command_schema.json`. The schema comes from `schemars`; the types inside a command that other crates own (shapes, resource bodies, patches) are described in `crates/kadakas-core/src/schema.rs`.
- After you add or change an API method, renew the Python package: `KADAKAS_BIN=<your binary> python python/tools/codegen.py`. The test `python/tests/test_drift.py` fails when `python/kadakas/_api.py` and `describe` differ.

## Many documents in one process

A host can run many `Session` values in one process (PLAT-12). The rules:

- A session owns all of its state. A library crate has no `static` with document data. A `static` is allowed for an id allocator (the editor revision, the GUID counter) or for a table that never changes.
- A big text in a resource (the file of a plant image) is shared text (`SharedText`, an `Arc<str>`): a clone is cheap and the check of equality stops at once for the same text. Do not read such a file at every change; read it where it first arrives and keep the answer (see `ImageResource::verify_data`).
- Every cache is per session and bounded: `RevCache` for one entry by revision, `LruCache` for many. The site analysis cache is in `SiteCache`, which `Session` owns.
- `SessionLimits` sets the limits of one session: objects, memory, undo history bytes, request time, render picture size, plant picture file size and pixels (`picture_bytes`, `picture_pixels`) and so on. `Session::set_limits` changes them. A call that changes the document and breaks the object or memory limit is undone. A session that is over the limit without a change only drops its caches, so reads and saves keep working. `document.load` and `file.open` refuse a document that does not fit before they replace the open one. The estimate counts the objects, the undo steps and the change log.
- Two process-wide values are explicit exceptions for the UI: the active language (`kadakas-l10n`) and the current units (`kadakas-units`). They are preferences of the person. They are not document data, and an API session never reads them.
- New ids are UUIDv7 (`Document::alloc_id`). A web build must call `kadakas_core::uid::set_host_seed(ms, salt)` once before it makes a document (the app does this in `new_web` with `Date.now()` and `crypto.getRandomValues`), because `std` has no clock or process there. Tests and fixtures use `Document::with_sequential_ids()` or `kadakas serve --sequential-ids`.

## Toolchain

- The minimum Rust version is 1.96 (`rust-version` in the workspace `Cargo.toml`). wasmtime 49 needs it. Use the current stable toolchain; CI uses `stable`.
- Raise `rust-version` only when a dependency needs a newer compiler, and then to the lowest version that works, not to the newest.

## Quality rules

- Write unit tests for all logic. Add property tests (proptest) where random input can find bugs.
- Tests must not depend on the speed of the machine. CI runs them in debug builds, under coverage instrumentation and on busy runners, so a wall-clock limit fails by chance. Check the work done: count cache builds and hits (`SiteCache::analyses_computed`, `AnnotCache::builds`, `section_cache.builds`), geometry operations, shared instances (`Arc::ptr_eq`, mesh and instance counts) and output sizes. A test of a time limit (a script, a download or a converter that must be stopped) asserts the `Timeout` error, not the time it took. Waiting loops with a deadline are fine: they fail only when something never happens.
- A test where speed is the point is a perf test. Mark it `#[ignore = "timing test: run in release mode"]` and put it in `tests/perf.rs` or in a test whose name starts with `perf_`. CI runs the perf tests in release mode (job `perf`): `cargo test --release -p kadakas-core -p kadakas-site -p kadakas-api -- --ignored`. Run them yourself after you change a hot path. A new crate with perf tests goes in that command.
- Add Python behavior tests in `tests/behavior` for features that agents can reach through the API.
- Keep line coverage of each library crate at 95% or more (`cargo llvm-cov -p <crate> --summary-only`). CI enforces it with `scripts/coverage.sh` (all crates, or name some: `scripts/coverage.sh print api`). The `kadakas-cli` crate has a bar of 90% (its tests run the binary). The app crate (egui and wgpu windows) is left out.
- Dependencies must have a permissive license. CI runs `cargo deny check` (settings in `deny.toml`). After you add or update a dependency, run `scripts/notices.sh` and commit `THIRD_PARTY_NOTICES.txt`; the installers ship it (`packager.toml`, `resources`).
- Attribution: every new dependency, asset (icon, font, image), dataset or cited formula or standard updates `docs/attribution.md`. Crates go in `THIRD_PARTY_NOTICES.txt` through `scripts/notices.sh`; other items go in `docs/notices-extra.txt`, which the script appends. Reviewers check this. Name a trademark only with its owner's note (see `docs/attribution.md`).
- Test helpers must run on Windows too: use `target/debug/kadakas.exe` there, and read pipes with a thread and a queue, not with `select`.
- Every text that a person reads in the UI goes in a `.ftl` file under `crates/kadakas-l10n/locales/en-US/`, with the Estonian text in `locales/et/`. In code, use `tr!("id")` and never a string literal in an egui call. A number that a person reads or types goes through `kadakas-units` (`current()` and `DragValueExt`). Do not write `format!("{:.1} mm", x)` in the UI. API and JSON-RPC texts stay English. `cargo test -p kadakas-l10n` is the CI check for this rule. See `docs/localization.md`.
- Workspace profiles (PLAT-04): a new key action, menu command or tool must work in every profile. Add a new `Action` to the Vectorworks-like defaults (`Action::defaults`); a profile file may leave it out and takes the default. Add a new menu command to `Cmd` in `profile/layout.rs` and to `default_menus`. A UI text that a profile may rename (a term such as "Class") stays a normal message; the overlay replaces it by id. Run `UPDATE_PROFILES=1 cargo test -p kadakas-ui-core` when the default profile changes, and commit `crates/kadakas-ui-core/profiles/vectorworks.json`. Built-in profile texts need en-US and et (`profiles/autocad.<tag>.ftl`); `cargo test -p kadakas-ui-core` checks them against the messages.
- Reject numbers that are not finite. Saved files must always open again.
- Plain English in documents and comments: short sentences, active voice.

## Checks before a commit

```
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p kadakas-units -p kadakas-l10n -p kadakas-core -p kadakas-crs -p kadakas-plants -p kadakas-collab -p kadakas-collab-ws -p kadakas-render -p kadakas-api -p kadakas-site -p kadakas-annot -p kadakas-x3d -p kadakas-ui-core -p kadakas-plugin-abi -p kadakas-plugin -p kadakas-feedback -p kadakas-app --target wasm32-unknown-unknown
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
