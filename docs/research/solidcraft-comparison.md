# SolidCraft compared with Kadakas

Date of research: 2026-10-24. Method: public web pages and a shallow read-only clone of the source.
The clone was never built, run or tested. No SolidCraft code is in this repository.
Sources are given as URLs or as paths inside the clone of https://github.com/tangency-labs/solidcraft at commit 9a737bd.
Anything not checked is marked "unverified".

## 1. What SolidCraft is

### The site solidcraft.pro is not the project

- solidcraft.pro is an unofficial community guide. It says so on its own page (https://solidcraft.pro/).
- The project sites are https://solidcraft.io and https://docs.solidcraft.io (named on solidcraft.pro).
- The source is https://github.com/tangency-labs/solidcraft. It moved from github.com/bherbruck/solidcraft at v0.15.0 (release notes, unverified mirror).
- The guide site has its own text and screenshots. Its licence is unknown. We read it for facts only.

### Facts

| Item | Finding | Source |
|---|---|---|
| Domain | Parametric 3D mechanical CAD (sketch, feature timeline, assemblies, sheet metal, 3D printing) | README.md |
| Users | Makers, 3D printing, product design, plus AI agents | README.md, solidcraft.pro |
| Stack | Pure Rust, egui and wgpu, wasm web build, headless CLI | README.md, Cargo.toml |
| Kernel | Own B-rep layer on the truck crates (Apache-2.0). A replacement called solidcore is in progress | NOTICE, CONTRIBUTING.md line 366 |
| Solver | Own Levenberg-Marquardt constraint solver | crates/sketch/src/lib.rs |
| 2D and 3D | 3D first. 2D is sketches, DXF import and export, view-to-DXF export. Drawings are deferred | ROADMAP.md, docs/view-dxf.md |
| File formats | Own .solidcraft JSON. STEP, IGES, 3MF, STL, OBJ, glTF, DXF, SVG, URDF. FreeCAD and Rhino import | README.md, crates/io/src |
| Scripting | JSON command scripts, CLI, MCP, a control protocol. Wasm plugins are a design document only | docs/plugins.md, docs/control-protocol.md |
| AI support | MCP server in crates/mcp. Agent skills in skills/. AGENTS.md and CLAUDE.md in the root | crates/mcp, skills/ |
| Platforms | Windows, macOS, Linux, browser. Packages: MSI, DMG, AppImage, deb, rpm, Snap, Flatpak, winget | README.md, packaging/ |
| UI toolkit | egui. About 157k lines in crates/ui-egui | counted in clone |
| Languages in UI | 25 locale files, for example cs, da, de, es, fr | assets/locales |
| Size | About 545k lines of Rust. doc 53k, engine 112k, kernel 95k, ui-egui 157k, sketch 24k, io 12k, mcp 4k | counted in clone |
| Tests | About 6,400 test attributes in 705 files. Reference "oracle" cases against analytic or FreeCAD values. Hostile-input fuzz. Headless UI scenarios | docs/oracle.md, ROADMAP.md |
| CI | 14 workflows: CI, release, nightly kernel, packaging lint, winget, Snap, Flatpak, Docker | .github/workflows |
| Docs | A docs book, 70 plus design notes, a roadmap with agent-hour estimates | docs/, ROADMAP.md |
| Maturity | Repo created 2026-10-07. About 2,090 commits in 17 days. About 9,800 stars and 1,200 forks. 3,070 open issues. Release v0.21.0 | GitHub API, solidcraft.pro |
| Team | Owner org tangency-labs, original author bherbruck. About 158 contributors counting anonymous. Sibling projects in the "storytold" family | GitHub API, AGENTS.md |
| Governance | Owner decisions in the roadmap. Discord. Sponsors page. No steering group found (unverified) | ROADMAP.md, docs/sponsors |
| Code origin | Press coverage calls it AI-assisted ("vibe-coded"). The repo has AGENTS.md rules for agents and roadmap time in "agent hours" | Fabbaloo article via search, ROADMAP.md |
| Self-assessment | "Early but usable for simple parts" | README.md |

The commit pace and the agent-hour roadmap show heavy agent authorship. Treat code quality as unverified.

## 2. Licence findings

### Repository licence

- Root files: LICENSE-MIT and LICENSE-APACHE. Standard texts. File sizes 1,080 and 10,174 bytes.
- LICENSE-MIT starts "MIT License, Copyright (c) 2026 SolidCraft contributors".
- NOTICE says: "Licensed under the MIT License or the Apache License, Version 2.0, at your option".
- The workspace sets `license.workspace = true` in all 16 crates. The value is unverified in the root Cargo.toml but the NOTICE text is clear.
- GitHub reports the SPDX id as Apache-2.0 only because it cannot show a dual licence. The real grant is MIT OR Apache-2.0.
- Contributions: CONTRIBUTING.md line 397 says contributors agree to MIT OR Apache-2.0. No CLA. No DCO sign-off found.
- NOTICE file: present. 2,297 bytes.

### Third-party code in the repository

| Item | Licence | Where |
|---|---|---|
| truck crates (modeling, shapeops, meshalgo, stepio, topology) | Apache-2.0, RICOS Co. Ltd. | used as libraries. NOTICE |
| vendor/truck-shapeops, truck-geometry, truck-meshalgo, truck-stepio | Apache-2.0 with local changes marked "SolidCraft:" | vendor/README.md |
| vendor/egui-wgpu, vendor/matext4cgmath | Own licences. Unverified, read each LICENSE before use | vendor/ |
| Code adapted from CADCraft and GridCraft (siblings) | MIT OR Apache-2.0 | ATTRIBUTION.md, NOTICE |
| Release packaging adapted from PhotoCraft | Apache-2.0 | NOTICE |
| Radiance RGBE reader in crates/render/src/lighting/hdr.rs | Adapted from image 0.25.10, MIT OR Apache-2.0 | NOTICE |
| All Cargo dependencies | Listed in THIRD-PARTY-LICENSES.txt, generated by `cargo xtask licences`. cargo-deny config in deny.toml | repo root |

### Assets

- Fonts: IBM Plex Sans and Mono, Liberation Sans, BIZ UDPGothic. All SIL OFL 1.1, unmodified, with licence text beside each file.
- Test files: 3DBenchy model (public domain). STEP files from dan335/step-conformance (CC0-1.0).
- Rover recordings: model by 0xlillith, lettering follows Ubuntu Light (Ubuntu Font Licence 1.0). Used in a GIF only.
- Icons: drawn in code (crates/ui-egui/src/icons.rs). Policy says all assets are original or openly licensed. `cargo xtask assets` checks the list.
- Datasets: none found beyond materials and appearance JSON in assets/. Source of those values is unverified.

### Proprietary derivation and clean-room status

- AGENTS.md calls the project clean-room. It forbids data, ids or files from Fusion, SolidWorks or Onshape.
- ATTRIBUTION.md bans Autodesk icons, images, fonts, templates and help text.
- It also forbids reading GPL code (FreeCAD, OpenCASCADE, SolveSpace and others). FreeCAD runs only as an external oracle.
- Press and the project call it a "Fusion-style" clone. The UI layout (ribbon, timeline) follows common CAD convention. Whether it copies Fusion command names is unverified.
- Their rules are policy. They do not prove provenance. Agent-written code has an unknown training-data origin.
- Our own rule is stricter. We study public docs only for SketchUp and Vectorworks.
  SolidCraft is a different product class (mechanical), so product-use rules do not touch it.

## 3. Differences table

| Area | Kadakas | SolidCraft |
|---|---|---|
| Domain | 2D landscape design: plots, plants, irrigation, sites | 3D mechanical parts and assemblies |
| Workflow | Draw, annotate, sheet layout, print | Sketch, constrain, feature timeline, rebuild |
| Terminology | Plots, layers, plants, sheets, annotations | Sketch, feature, timeline, joint, configuration |
| Data model | Document of objects. Every change is an undoable command. Millimetres, Y up | Parametric feature timeline with persistent face names. Every operation is a command |
| Geometry kernel | 2D geometry, GIS and CRS, small own 3D modeler | B-rep on truck, own solver, solidcore planned |
| Rendering | egui, SVG render, 3D view crate, wgpu | egui and wgpu, compute-shader tessellation, PBR render workspace |
| Interop | DXF in and out, DWG by converter, Shapefile, GeoJSON, IFC in and out, OBJ, STL, glTF | STEP, IGES, 3MF, STL, OBJ, glTF, DXF, SVG, URDF, FreeCAD, Rhino. No IFC, no GIS |
| Agent API and MCP | JSON-RPC over stdio, TCP, MCP server in kadakas-cli | MCP server, control protocol, skills folder, agent chat doc |
| Scripting | Python package, JS script, sandboxed Wasm plugins (shipped) | JSON command scripts. Wasm plugins designed only |
| Collaboration | kadakas-collab and WebSocket crate | None found (unverified) |
| Packaging and updates | dmg, exe, msi, AppImage, deb. Unsigned. Update crate | Many more targets, winget, Snap, Flatpak, Docker. Unsigned. In-app update |
| Localisation | kadakas-l10n crate | 25 locale files, i18n check in xtask |
| Testing and quality | Workspace tests, doc-load benchmark | 6,400 tests, oracle cases, fuzz, mutation testing doc, layer checks, asset checks |
| Licence | Apache-2.0 OR MIT | MIT OR Apache-2.0 |
| Community | Small | Discord, 158 contributors, sponsors, trending |

## 4. What Kadakas can learn

Ranked by value for effort.

1. Asset attribution gate. A CI check fails when an asset has no row in an attribution table. Effort: small. It protects our clean-room claim. See xtask/src/assets.rs.
2. Oracle tests. Reference cases with values from maths or an external tool, compared in CI. Effort: medium. Good for geometry such as areas, slopes, grading volumes.
3. Hostile-input fuzz over every command. Effort: medium. Fits our command model and our DXF and IFC readers.
4. Layering check. A tool that fails when a crate imports from a higher layer. Effort: small. Our core already has "no UI code", so enforce it. See xtask/src/layers.rs.
5. "Never crash" rules. No unwrap, expect or panic outside tests, forbid unsafe, errors say "not supported yet". Effort: small as lints, large to clean up.
6. Generated feature list. A command registry that writes docs/features.md. Effort: small. Keeps docs and code in step.
7. Agent skills folder with a modelling guide for MCP clients. Effort: small. See skills/.
8. Agent instruction files (AGENTS.md) with licence and clean-room rules. Effort: small.
9. Headless UI scenarios, and `snapshot` rendering of a document to PNG for docs. Effort: medium.
10. Crash capture and recovery docs. We already have kadakas-crash. Compare design. Effort: small.
11. Wider packaging: winget, Flatpak, Snap. Effort: medium. Signing is still open for both projects.
12. Community: Discord, roadmap with milestones, issue templates, PR template, release-notes template. Effort: small.
13. Locale count and an i18n completeness check. Effort: medium.
14. Wasm plugin design. Their plugin document is still a design. Ours ships. Share lessons, not code. Effort: none.

Do not copy: the Fusion-like ribbon and timeline. Our users are landscape designers.

## 5. Code reuse

### Licence position

- Both licences are compatible with ours. Apache-2.0 OR MIT lets us pick MIT for copied files. MIT asks only for the copyright and licence text.
- Choosing Apache-2.0 for a file has more duties. The next section lists them.
- Code from truck or egui-wgpu inside vendor/ keeps its own licence, so read that file first.
- Nothing here is GPL, LGPL, AGPL or CC-BY-SA. Fonts are OFL. Those need their own notices.
- The guide site solidcraft.pro has no stated licence. Do not copy its text or images.

### What Apache-2.0 requires if we copy a file

1. Give a copy of the Apache-2.0 licence to recipients.
2. Keep every copyright, patent, trademark and attribution notice in the source file.
3. Mark changed files with a prominent notice that we changed them.
4. Carry the content of the NOTICE file into our distribution. The relevant text is the "Third-party material" part of their NOTICE, trimmed to what we copy.
5. Put that content in THIRD_PARTY_NOTICES.txt and docs/attribution.md.
6. Show it in the About dialog, where our existing notices appear.
7. Our own changes stay under our licence. The NOTICE text does not change the licence.

### Patent warning

- Apache-2.0 section 3 grants a patent licence from each contributor. It ends if we start patent litigation over the work.
- Contributors here include agents and many people. We cannot check their patent rights. This is the same risk as any Apache dependency.
- MIT has no patent clause. That is a reason to pick MIT for small files.

### Candidates

All paths are in the clone. Provenance is "agent-assisted, policy says clean-room". Review each file by hand before any port.

| Candidate | Why | Fit | Risk |
|---|---|---|---|
| crates/io/src/dxf_out.rs and dxf.rs | DXF writer and reader. Compare against our kadakas-io | Medium | Their entity set is mechanical. Check against our tests in docs/research/dxf-real-world.md |
| crates/io/src/gltf_in.rs, obj.rs, threemf.rs | Mesh readers | Low. kadakas-meshio exists | Duplicate work |
| crates/io/src/safe_write.rs | Atomic file write | Good | Small file. Check ours first |
| crates/mcp/src (about 4k lines) | MCP server over a command registry | Low. We have one in kadakas-cli | Compare for protocol gaps only |
| crates/doc/src/expr.rs | Unit-aware expression parser | Medium, for dimension fields | Needs our unit model |
| xtask/src/assets.rs, layers.rs, licences.rs | CI gates | Good | Port as a script. Rewrite for our layout |
| crates/space-mouse | 3D mouse HID support | Low for 2D | Unverified |
| assets/locales/*.json | Translations of mechanical UI | None | Wrong vocabulary |
| packaging/ and .github/workflows/release.yml | Adapted from PhotoCraft (Apache-2.0) | Medium | Chain of notices: PhotoCraft first |
| crates/sketch (solver) | Geometric constraints | Only if we add constraints | Large. Depends on their types |
| crates/kernel, vendor/truck-* | B-rep | None. Wrong domain | Large |

### Depend or copy?

- No SolidCraft crate is on crates.io (unverified). Crates depend on each other by path inside the workspace.
- A git dependency on a pre-1.0 project that changes daily is a poor choice. Names and formats move every release.
- The crates are not made as a library. Layers couple tightly (doc to engine to ui).
- Better: copy a single small file under MIT, with notice, and then own it. Or write our own from the public spec.
- For the solver, a git dependency is better than a copy if we ever want it. Pin a commit and review.
- truck is a real crates.io dependency. If we need B-rep, depend on truck directly, not on SolidCraft.

## 6. Recommendation: top 5 actions

1. Add an asset attribution gate and a licence gate to our CI. Port the idea, not the code.
2. Add a layering check and a no-panic lint set to protect the core crate.
3. Build oracle tests for landscape maths (areas, volumes, slopes) and a hostile-input fuzz target for DXF and IFC.
4. Write an AGENTS.md and a skills folder for MCP users, with our clean-room rules.
5. Do not copy code now. Re-check in six months, when the project and its provenance are clearer.

## 7. Risks

- Agent-written code has unclear provenance and unclear copyright. The project policy does not prove it.
- The project moves fast. Facts here are from 2026-10-24 and age quickly.
- The clone was shallow. Commit history and authors were not read.
- Vendored egui-wgpu and matext4cgmath licences are unverified.
- Counts of tests and lines come from simple greps in the clone. They are rough.

## 12. Business, server and collaboration

Facts are from 2026-10-24. The clone is shallow with one commit, so history and authors are unverified.

### A. Business

- The GitHub organisation is tangency-labs. The footer of solidcraft.io says "© Tangency Labs". The org page names no company, members or legal entity (github.com/tangency-labs).
- The legal form of Tangency Labs is unverified. The privacy page says the responsible operator is not yet configured (solidcraft.io/privacy).
- The repo moved from bherbruck/solidcraft. The update check still points to that name (crates/ui-egui/src/update.rs).
- The main author is probably bherbruck. His profile shows pinned tangency-labs repos and 16 GitHub sponsors. His employer is unverified (github.com/bherbruck).
- The storytold family is a separate GitHub org with CADCraft, GridCraft, VectorCraft and PhotoCraft. SolidCraft copies code from them under MIT or Apache-2.0 (NOTICE, ATTRIBUTION.md).
- Tangency Labs also publishes solidcore (Apache-2.0), fieldcraft, pcbcraft (GPL-3.0) and an empty solidcraft-plugins repo (github.com/tangency-labs).
- Funding: .github/FUNDING.yml lists GitHub Sponsors and Ko-fi for bherbruck, a person and not the company.
- Blacksmith sponsors the CI minutes (README.md, Sponsors section).
- The repo has no Open Collective, no dual licence, no paywall, no ads and no token. The licence is MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).
- The community site solidcraft.pro links to "SolidCraft online", PartBatch, pricing, terms, refund and account pages. It says it is unofficial and unaffiliated. The operator is unnamed.
- So a paid hosted service exists in the ecosystem, run by an unknown party. Its link to Tangency Labs is unverified.
- Telemetry: none found. The desktop app asks GitHub for the latest release once a day, sends no identifier, and has an off switch (crates/ui-egui/src/update.rs).
- Crash reports stay on the computer. The user sends them by hand as a GitHub issue draft (docs/crash-reports.md).
- The website has no analytics, no accounts and no cookies (solidcraft.io/privacy). It is static on Cloudflare Pages (github.com/tangency-labs/website).
- Trademark and domain owners are unverified. The README only disclaims CAD vendor trademarks (README.md, NOTICE).

### B. Server component

- The repo has no server, cloud, account, sync, registry or telemetry endpoint. Only static hosting exists.
- The web app is wasm. A Dockerfile serves the static site with nginx on port 8080 (packaging/docker/Dockerfile).
- The web build stores designs in the browser origin private file system. An autosave sits there too (apps/solidcraft-web/src/files.rs).
- The app updates itself through a service worker (crates/ui-egui/src/update.rs).
- The only network calls from the app go to GitHub releases and to the agent endpoint the user sets (apps/solidcraft/src/main.rs, docs/agent-chat.md).
- The MCP server and control API listen on loopback with a token (docs/mcp.md, docs/control-protocol.md).
- Updates for Flatpak come from a feed on docs.solidcraft.io (docs/compatibility.md).

### C. Sharing and collaboration

- Found no share links, comments, presence, CRDT, OT, WebSocket or file locking. The Cargo.lock has no sync crate such as yrs or tungstenite.
- Export and download of files is the only way to share. The crash lock file is per run and is not a document lock (ROADMAP.md).
- Undo and redo exist, with one step per agent message (ROADMAP.md). No named versions or history browser were found.
- The ROADMAP.md comments item is about parameter notes. Real collaboration is not on the roadmap (unverified beyond a grep).
- The file format is a JSON container. Its merge friendliness is unverified (crates/io/src/design.rs).

### Comparison with Kadakas

- Kadakas plans a separate collab server repo, the crate kadakas-collab and the protocol in docs/collaboration.md. SolidCraft has none of these.
- Kadakas has a server-ordered operation log, last-writer-wins per property, per-user undo and presence (feature-spec PLAT-07, PLAT-08).
- Kadakas has named versions, diff and BCF issues (REV-01, REV-02, REV-04). Shared ones live on the server (REV-09).
- SolidCraft has the same local-first stance and the same MIT or Apache-2.0 licence choice for the client.
- The Kadakas server sits in its own repo so its licence is chosen apart (feature-spec, platform section). SolidCraft has no such split.
- Kadakas needs hosting, accounts and operations cost. SolidCraft pays almost nothing and relies on GitHub, Cloudflare Pages and sponsors.

### What to learn

- Keep the client free of any account. Make every server feature optional, as the SolidCraft desktop app does.
- Copy the update check design. One public request, no identifier, an off switch, a silent failure.
- Publish a privacy page early. Name the operator and a contact. SolidCraft left both unset and looks unfinished.
- Keep crash reports local and send them by user action only.
- Own the name and the domain with a legal entity before launch. SolidCraft changed its repo name and feed addresses three times.
- Watch the unofficial paid hosting around SolidCraft. A third party sells the hosted service while the project does not. Plan trademark rules for Kadakas.
- Fund through sponsors and a paid collab server. A paid server fits the separate repo plan and keeps the client free.
