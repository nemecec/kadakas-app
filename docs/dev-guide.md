# Cadektor developer guide

This guide gives the rules for all work on Cadektor, by people and by AI agents.

## Product rules

- Cadektor is a clean-room CAD application for landscape architects. Its workflow and terms follow Vectorworks 2026, so Vectorworks users feel at home.
- Use only public information: help pages at app-help.vectorworks.net, product pages and the published shortcut list. Do not use the Vectorworks software itself.
- Write all text and code in your own words. Do not copy help text, icons or graphics. Do not use Vectorworks trademarks as names in the product.
- Before you design a feature, read `docs/research/vectorworks-public-features.md` and the related public help page. Match the tool modes, the parameters, the terms and the default keys.

## Architecture

| Crate | Content | Depends on |
|---|---|---|
| `cadektor-core` | Document model, commands, undo, geometry, curves, mesh | none of ours |
| `cadektor-render` | Camera, standard views, picking, CPU rasterizer | core |
| `cadektor-io` | DXF, Shapefile and GeoJSON import and export, CRS conversion (native only) | core |
| `cadektor-annot` | Data tags, legends and annotation output on top of the worksheet code | core, sheet |
| `cadektor-gis` | WMS, WMTS and WFS services, geoimages, tile cache, document CRS change, coordinate readout (native only; has the HTTP client) | core, io |
| `cadektor-ifc` | IFC4 export, STEP writer and parser (native only) | core, io, site |
| `cadektor-api` | JSON-RPC session and methods for agents | core, render, io, ifc, gis, annot (native only for io, ifc and gis) |
| `cadektor-cli` | `serve`, `mcp`, `render` | api |
| `cadektor-app` | egui and wgpu desktop app | api, render, core |

- Put each feature in its own crate or module, for example `cadektor-io` for file formats and `cadektor-site` for terrain. Keep the interface small.
- Core, render and api must build for `wasm32-unknown-unknown`. Keep file access, threads and GPU code out of them.
- All document changes go through `Command` values. The GUI, the API and scripts use the same commands, so every change can be undone.
- When you add a `Shape` variant or an API method, keep the edits to shared files small and in one place.
- Agents must reach every document feature through the API. Add an example to `describe`.

## Quality rules

- Write unit tests for all logic. Add property tests (proptest) where random input can find bugs.
- Add Python behavior tests in `tests/behavior` for features that agents can reach through the API.
- Keep line coverage of each library crate at 95% or more (`cargo llvm-cov -p <crate> --summary-only`).
- Reject numbers that are not finite. Saved files must always open again.
- Plain English in documents and comments: short sentences, active voice.

## Checks before a commit

```
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p cadektor-core -p cadektor-render -p cadektor-api -p cadektor-site -p cadektor-annot --target wasm32-unknown-unknown
cargo build -p cadektor-cli && uv run --with pytest --with ezdxf --with ifcopenshell pytest tests/behavior -q
```

All five must pass. End each commit message with the co-author line that the project uses.

## Parallel work in worktrees

- Use your own build folder: `CARGO_TARGET_DIR=<repo main folder>/target-<feature>`. Shared build folders mix binaries from different branches.
- Disk space is limited. Set `CARGO_INCREMENTAL=0` and `CARGO_PROFILE_DEV_DEBUG=0`. Run release and coverage builds only when you need them.
- Point the behavior tests at your binary: `CADEKTOR_BIN=$CARGO_TARGET_DIR/debug/cadektor`.
- Other branches add `Shape` variants, commands and API methods at the same time. Add catch-all arms where a match does not need every kind, and keep shared edits small.
- Before you finish, merge `main` into your branch, resolve conflicts, and run the checks again.
