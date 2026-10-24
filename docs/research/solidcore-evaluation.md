# SolidCore evaluation

Date: 2026-10-24. Scope: decide if Kadakas adopts SolidCore, the Rust port of SMLib, or learns from it.
Method: public repositories only, read as text. Nothing from them was built, run or copied.
Items marked "unverified" were not confirmed from a source.

## Summary

- SolidCore is real, large and moving fast. It is a faithful port of an exact NURBS B-rep kernel.
- It is not yet a usable product. No STEP, no tessellation path confirmed, no release above 0.0.1.
- The licence chain is clean on paper. Three points need a lawyer.
- Kadakas has no need for exact B-rep today. The recommendation is to port ideas only and watch the project.

## 1. What SolidCore is

### Location and identity

- SolidCore is not inside the SolidCraft repository. It lives in its own repository.
  Source: https://github.com/tangency-labs/solidcore (README.md).
- The SolidCraft checkout names it as a planned replacement for its current kernel.
  Source: SolidCraft CONTRIBUTING.md, section "Where help is wanted most".
- SolidCraft itself builds its kernel on the truck crates today, not on SolidCore.
  Source: SolidCraft NOTICE and Cargo.toml (truck-modeling 0.6, truck-shapeops 0.4, truck-stepio 0.3).
- The SolidCore workspace has three crates: `solidcore` (the kernel), `solidcore-ffi` and `difftest`.
  Source: SolidCore Cargo.toml.
- `solidcore-ffi` and `difftest` exist to compare the Rust port with the C++ original. Both are test-only.
  Source: SolidCore README.md, section "Releasing".

### Size and tests

- The `solidcore` crate holds about 492,000 lines of Rust. Measured by counting lines in `crates/solidcore`.
- The port plan tracks 261 units and about 388,000 upstream lines.
  Source: SolidCore plan/PROGRESS.md (generated file).
- plan/STATUS.md says all 261 units are ported. It lists 199 green and 61 partial units.
- The same file reports 554,873 difftest cases. It lists 550,446 passing, 0 failing and 4,186 deferred.
- Difftest runs the Rust code and the C++ original on the same inputs.
  The stated target is agreement to 1e-9 relative with identical topology.
- The README is out of date. It still says "about a third ported" and "nothing is usable end to end".
  Source: SolidCore README.md, section "Status". The newer plan files disagree with it.
- Counting `#[test]` attributes finds about 2,100 unit tests. Most of the assurance comes from difftest.
- Difftest needs the C++ reference build. That build is Linux-first. The README says macOS parity is not established.

### Public API and capabilities

The crate exports these modules. Source: crates/solidcore/src/lib.rs.

- `math`, `geom`, `nurbs`, `curve`, `surface` for NURBS and analytic geometry.
- `topology` with a `Brep` arena and typed ids for shells, faces, loops, edges and vertices.
- `intersect`, `boolean`, `blend` (fillets and chamfers), `offset`, `sweep`, `primitive`.
- `heal`, `mass` and `query`.

Intended scope, per the README: booleans, fillets, chamfers, offset and shell, sweeps and skins, primitives, healing and mass properties.

Not in scope, per ATTRIBUTION.md:

- OpenCASCADE translators, USD and Hydra integration, and Python bindings.
- The upstream STEP, IGES and JT conversion goes through OpenCASCADE. SolidCore leaves it out.
- So SolidCore has no STEP or IGES file reader. Treat this as confirmed absent.
- Tessellation exists upstream (`SmTess.h`). Whether the Rust port covers it is unverified.
- Upstream has no sketch solver. SolidCore has none.

### Maturity and limits

- The crate version is 0.0.1. The README says it stays 0.0.x until booleans and fillets work end to end.
  Source: crates/solidcore/Cargo.toml and README.md.
- Several fillet, primitive, boolean and sweep units were still open pull requests on 2026-10-24.
  Source: plan/PROGRESS.md, section "In progress".
- The snapshot is two days old. The main branch had a merge on 2026-10-24. The pace is very high.
- Many contributors, including AI agents, work in parallel. The README invites agents to take units.
- Upstream known issues carry over to the port. Examples are `heal_brep` reporting success after a failed repair
  and `shell_brep` returning a non-solid. Source: upstream KNOWN_ISSUES.md.
- Upstream also lists a crash in `stitch_brep_with_face` and healing bugs on closed surfaces.
- Performance claims: none found. Unverified.
- The author notes recursion depth risk on wasm. Consumers need a wasm stack of 8 MiB.
  Source: README.md, section "WebAssembly stack size".

### Standalone use

- `solidcore` depends only on `thiserror`. Source: crates/solidcore/Cargo.toml.
- It has no features. No truck dependency.
- It uses safe Rust. `unsafe_code` is denied in the kernel. Only the FFI crate allows it.
- It targets `wasm32-unknown-unknown` and has a wasm check command. A wasm build was not run here.
- Edition 2021, Rust 1.85. Kadakas uses edition 2024 and Rust 1.96. Both mix without trouble.
- There is no single stable entry point yet. Unverified whether the public API is settled.

## 2. Provenance and licence

### The chain in plain words

1. SMLib began as a commercial kernel from Solid Modeling Solutions. It builds on the NLib NURBS library
   by Wayne Tiller and Les Piegel. Solid Modeling Solutions held the rights to NLib.
   Source: https://docs.nvidia.com/smlib/manual/nlib/introduction/index.html and a web search on 2026-10-24.
   Older SMLib documentation said licences went only to legacy customers.
2. NVIDIA published SMLib as part of USD BRep under Apache-2.0 in October 2026.
   Source: https://github.com/NVIDIA-Omniverse/usd-brep (LICENSE, created 2026-10-08, GitHub reports Apache-2.0).
   Copyright line: "Copyright (c) 2020-2026 NVIDIA CORPORATION & AFFILIATES".
3. SolidCore is a Rust translation of SMLib and SM_API from commit `c9c5979` of that repository.
   It is Apache-2.0 only. Source: SolidCore LICENSE, NOTICE, README.md and workspace Cargo.toml.

### What the files say

- SolidCore LICENSE is the upstream Apache-2.0 text, copied verbatim. It keeps the NVIDIA copyright line.
- NOTICE calls the port "a derivative work" and lists the upstream notice and SPDX header text.
- NOTICE lists the upstream third-party notices. It says none of those components is used by SolidCore.
- ATTRIBUTION.md lists the upstream paths used and the paths left out.
  Left out: OCCT translators, USD and Hydra layers, Python bindings, proposals.
- Per ATTRIBUTION.md, every ported Rust file starts with a header naming its upstream source files.
  I checked the policy text. I did not check each file header. Unverified for all files.
- Upstream source files carry `SPDX-License-Identifier: Apache-2.0`. Source: usd-brep `source/SMLib/inc/SmBrep.h`.
- Description of the method: line-by-line translation checked by difftest. Contributors include AI agents.
  Source: README.md, section "Contributing", and AGENTS.md.
- Port code keeps upstream naming and constants "digit-for-digit". Source: workspace Cargo.toml lint comments.
  This confirms close translation, not a clean-room rewrite.

### Does the "open-source" claim hold?

- The licence text is real Apache-2.0 and OSI approved. For the SMLib part, yes on its face.
- The repository says NVIDIA-authored portions are Apache-2.0 "except where otherwise noted".
  Source: usd-brep THIRD_PARTY_NOTICES.md. The repo-bootstrap scripts are MIT. OpenUSD has its own licence.
- SMLib is derived from a commercial product. The history above shows it.
- NVIDIA presumably holds the rights it licenses. How it got them from Solid Modeling Solutions is not public in
  the files read. Unverified.
- Apache-2.0 section 3 grants a patent licence only for patents NVIDIA owns and that its contribution reads on.
  It gives no protection from patents of other holders. Old kernel algorithms possibly touch third-party patents. Unverified.
- Apache-2.0 ends the patent licence if the user sues over the work.

### Is "MIT OR Apache-2.0" legitimate for the port?

- The port itself is Apache-2.0 only. Its own files do not claim MIT.
- The SolidCraft application is "MIT OR Apache-2.0". That applies to SolidCraft's own code.
  Source: SolidCraft NOTICE.
- If SolidCraft links SolidCore, the combined work carries Apache-2.0 terms from SolidCore. The "OR MIT" choice then holds
  only for SolidCraft's own files. A lawyer needs to confirm how SolidCraft words this.
- Apache-2.0 allows relicensing of derivative works. The derivative keeps the Apache-2.0 text, the notices
  and a record of changes. The port appears to do this.
- Kadakas workspace licence is "Apache-2.0 OR MIT". Source: kadakas-app Cargo.toml.
  A Kadakas build that includes SolidCore is Apache-2.0 in effect.

### What a lawyer needs to confirm

1. NVIDIA holds all rights in SMLib, including the NLib parts, and has the right to license them under Apache-2.0.
2. No third-party patent claims cover the kernel algorithms. Apache-2.0 does not grant them.
3. The Rust port is a valid derivative work. This covers the AI-assisted translation, contributor terms
   and copyright ownership of the new Rust text.
4. Whether `SolidCore` and `SMLib` names or the NVIDIA mark need any rule beyond the attribution already given.

This document is research. It is not legal advice.

## 3. Comparison with Kadakas

Kadakas sizes (counted lines of Rust): surface model 29,800 in `kadakas-core/src/surface`, meshio 4,100, x3d 4,200, ifc 15,100.
Sources: docs/modeler.md, docs/modeler-limits.md and the crates named.

| Capability | Kadakas today | SolidCore | Winner and notes |
|---|---|---|---|
| Surface model | Radial-edge faceted model. Faces are planar. Curves are polylines with soft and smooth edge flags | Exact NURBS and analytic surfaces with trimmed faces | SolidCore for exactness. Kadakas for simplicity and editing speed |
| Push/Pull, Follow Me, Offset | Done in `surface/`, tested, tuned to 50k-face limits | Sweeps and offset exist. No interactive editing layer | Kadakas |
| Booleans (3D) | Union, subtract, intersect, trim, split in `surface/solid/` on triangle meshes | Exact B-rep booleans. Some units still partial | Kadakas for robustness today. SolidCore if finished |
| Booleans (2D) | `edit2d/boolean.rs` on i_overlay | Not applicable | Kadakas |
| Fillets and chamfers | None on solids. Soft edges only | Large fillet solver, partly open | SolidCore, once done. Landscape need is low |
| Offsets and shells | Planar offset of faces and curves | Exact offset and shell. Known upstream shell bug | Mixed |
| Curved surfaces | Approximated by triangles | Exact | SolidCore |
| STEP import and export | None. IOP-11 is open (docs/feature-spec.md) | None. Upstream excluded STEP translators | Neither |
| Part 21 text parser | `kadakas-ifc/src/step.rs`, 790 lines, used by IFC | None | Kadakas |
| Mesh I/O | OBJ, STL, glTF in `kadakas-meshio`. X3D in `kadakas-x3d` | None | Kadakas |
| IFC | `kadakas-ifc`, 15k lines | None | Kadakas |
| Tessellation | Not needed. Model is already facets | Present upstream. Port status unverified | Kadakas |
| Sketch solver | Not part of this question | None | Neither |
| Performance | Measured in docs/modeler-limits.md | No numbers published | Kadakas, for lack of data |
| Wasm | Core targets web (docs/web.md) | Targets wasm. Needs 8 MiB stack | Tie. Check the stack setting |
| Determinism | Kadakas aims for it. Edits are replayable | Stated goal. No global state, difftest checks | Tie |
| Maturity | Product code in use | 0.0.1, changing daily | Kadakas |

### Do landscape tasks need exact NURBS?

| Task | Needs exact curves? | Why |
|---|---|---|
| Terrain | No | Terrain is a triangle mesh in survey data and in GIS |
| Paving and patios | No | Planar faces and polygon outlines |
| Walls and steps | No | Boxes and extrusions |
| Planting beds | Rarely | Circles and arcs as fine polylines look right |
| Pergolas and fences | No | Straight members |
| Water features and pools | Sometimes | Curved shells and fillets help. Facets work at 1 mm tolerance |
| Manufacturer products | Yes, for import | STEP files carry exact surfaces. Import tessellates on load |

Conclusion: exactness is a nice-to-have for a few shapes. A tessellated STEP import covers most of the value.

## 4. Benefits and costs

### Three adoption paths

| Path | Effort | Risk |
|---|---|---|
| Depend on the crate | Low to start. High to bridge a B-rep to our faceted model | High churn. The API is not stable. Pre-1.0 |
| Vendor the source | Very high. About 490k lines to own | Heavy review load. Same provenance questions |
| Port ideas only | Low and local | Low. We need to avoid copying code |

### Costs of depending or vendoring

- Binary size and build time: unverified. About 490k lines of Rust suggest a large increase. Measure before any decision.
- Churn: the README says units land daily and layers are tightly coupled.
- Attribution: Apache-2.0 needs the licence text, the NOTICE content and change records. Kadakas has `docs/attribution.md`.
- Combined licence: the shipped build is Apache-2.0 in effect. We lose the "OR MIT" claim for that build.
- Clean-room claim: the clean-room rule in our memory concerns SketchUp and Vectorworks products. SolidCore is not those.
  The claim stays valid. We still do not read or copy SolidCraft application code that mirrors those products.
- File format: B-rep data needs a new store beside the faceted surface model. This breaks the single-model design.
- Agent API: every tool in `kadakas-api` assumes faceted edits. Exact solids need new commands and new limits.
- Quality: our own engine is tested against our rules. A ported kernel brings its own failure modes (see KNOWN_ISSUES).

### What landscape users gain

- Exact import of STEP products with cylinders, tori and splines.
- Fillets and chamfers on solid objects, such as furniture, planters and water features.
- Exact measures of volume and area.
- Clean export to STEP for makers and stone cutters.

### What we lose

- Simplicity, small build, and one model that every tool understands.
- Control over fixes. Upstream bugs wait on upstream.

## 5. Recommendation

Port ideas only. Do not depend on or vendor SolidCore now. Re-check in six months.

### Ranked actions

1. Close IOP-11 with a small STEP reader that tessellates on load. Extend `kadakas-ifc/src/step.rs` for the Part 21 layer.
2. Test `truck-stepio` plus `truck-meshalgo` (both Apache-2.0) behind a cargo feature as a import-only path. Measure size and wasm build.
3. Write the licence questions of section 2 into one note for a lawyer before any use of SolidCore code.
4. Study the SolidCore plan files and the public upstream docs for algorithm ideas, such as healing and tolerance handling.
   Write our own code. Do not copy Rust or C++ text.
5. Re-check SolidCore on 2027-04-24. The gate: a tagged release 0.1 or higher, working booleans and fillets, a STEP path, a wasm build report.
6. Skip fillets on solids until a user need shows up.

### truck compared with SolidCore

- truck is a B-rep crate family under Apache-2.0 by RICOS. Source: crates.io `truck-modeling` 0.6.0, `truck-shapeops` 0.4.0, `truck-stepio` 0.3.0.
  All three were last published in September 2024. Source: crates.io API on 2026-10-24.
- It has a STEP reader and writer (`truck-stepio`), tessellation (`truck-meshalgo`) and booleans (`truck-shapeops`).
- It has no fillets. Source: SolidCraft CONTRIBUTING.md says "truck has no fillet operation".
- SolidCraft vendors and patches `truck-shapeops`, `truck-geometry`, `truck-meshalgo` and `truck-stepio`.
  Source: SolidCraft vendor/README.md and NOTICE. This points to weak boolean and tessellation robustness. Unverified in detail.
- Verdict: truck fits better for a small, import-focused step today. It is published, has a clear licence and a STEP path.
  SolidCore has the stronger design and wins later if it ships. It has no release and no STEP.

### STEP parser candidates

| Candidate | Licence | Notes |
|---|---|---|
| `ruststep` 0.4.0 | Apache-2.0 | Part 21 and EXPRESS-based schema code. Last published September 2024. API size unverified |
| `truck-stepio` 0.3.0 | Apache-2.0 | Reads solids into truck shapes. Pulls in truck |
| `kadakas-ifc/src/step.rs` | Ours | Part 21 values, entities, parser, writer. No geometry schema for AP203 or AP214 |
| `iso-10303` 0.5.0 | Unverified | Last published 2021. Not recommended |

Smallest plan: parse with our own `step.rs`. Map `ADVANCED_BREP_SHAPE_REPRESENTATION` faces to triangles
for planar, cylindrical and conical faces first. Fall back to `truck-stepio` for splines.

## Sources

- https://github.com/tangency-labs/solidcore (README.md, ATTRIBUTION.md, NOTICE, LICENSE, plan/PROGRESS.md, plan/STATUS.md, Cargo.toml)
- https://github.com/tangency-labs/solidcraft (NOTICE, CONTRIBUTING.md, Cargo.toml, vendor/README.md)
- https://github.com/NVIDIA-Omniverse/usd-brep (LICENSE, THIRD_PARTY_NOTICES.md, KNOWN_ISSUES.md, CHANGELOG.md, .agents/docs/OVERVIEW.md)
- https://docs.nvidia.com/smlib/manual/nlib/introduction/index.html
- https://develop3d.com/cad/nvidia-launches-usd-brep-geometry-kernel-for-solid-modelling/
- https://crates.io/crates/truck-stepio, https://crates.io/crates/ruststep
- docs/research/solidcraft-comparison.md in this repository
