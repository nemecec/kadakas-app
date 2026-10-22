# Limits of the modeler: measurements and recommendations

This document is the measured base for the limits of surface models (MOD-06) and of groups and components (MOD-07). It is the second report. The first one measured the code of commit `63a93623`. This one measures the code with the spatial index for `add_edges` and with the new guards. The project rule is that limits come from measurements and are chosen with the user. So this document changes no limit value. It adds five guard constants, and every recommendation waits for the user.

A surface model is the 3D geometry of one object: vertices (points), edges and faces. A component is a definition of 3D parts with many instances. A guard is a limit that protects time or memory and is not a modeling rule.

## Machine and method

- Machine: Apple M1 Pro, 10 cores, macOS. Release builds. One scenario per process, run under `/usr/bin/time -l`.
- Each number is the median of 5 runs. Three runs were used for some slow cases, and one run for the slowest. The tables say so.
- The machine was shared with other work. The load average (`uptime`) was 2.2 to 3.9 for the guard and raised-limit runs. It was 3 to 9 for the Explode runs of the old code. The exact lines are in the result files of the campaign.
- Times are CPU time (user plus system) unless a table says wall. Heap numbers come from a counting allocator and do not depend on load.
- "Drawn" means built with `Edit::AddEdges`. "Constructed" means built with `SurfaceModel::from_parts`, which makes the same data as a loaded file without drawing time.
- Raised limits: the cfg `kadakas_measure` (set with `RUSTFLAGS`, not a cargo feature) lets a measurement set a factor for the four count limits. These are vertices, edges, faces and loop vertices. Without the cfg the factor is 1 and nothing changes it. No dependency can turn the cfg on. The raised runs use the factor 3. The edit limit of 10,000 edges stays as it is.
- Code: `crates/kadakas-core/tests/limits_measure.rs` (+ `limits_common.rs`, `limits_measure.py`, `limits_campaign.sh`), `crates/kadakas-ui-core/tests/limits_measure.rs`, `crates/kadakas-props/tests/limits_measure.rs`. The stages of the second campaign are `big`, `big2`, `guards`, `explode`, `collab2` and `collab3`. The first campaign stages `a1` and `a2` were run again.
- "Old" in a table is main at commit `114a7d50`, which has the index. Where the table says so, it is the first report.

## A. Surface model

### A1. Which limit binds first

The limits are the same as in the first report. They are 50,000 vertices (V), 100,000 edges (E) and 50,000 faces (F). The other two are 200,000 loop vertices and 10,000 edges in one edit. Loop vertices (L) are the sum of the corners of all faces. The sizes at which each shape stops did not change. The table gives the largest model that fits.

```text
Shape                              V       E       F       L        Binds first
---------------------------------  ------  ------  ------  -------  -----------------------------------
Flat quad grid 222 x 222           49,729  99,012  49,284  197,136  vertices (the others are within 1%)
Triangulated terrain 158 x 158     25,281  75,208  49,928  149,784  faces
Noisy terrain 158 x 158            25,281  75,208  49,928  149,784  faces
Triangulated UV sphere, 112 rings  24,866  74,592  49,728  149,184  faces
Spiky sphere, 112 rings            24,866  74,592  49,728  149,184  faces
6,250 separate cubes               50,000  75,000  37,500  150,000  vertices
```

A triangulated mesh has 3 edges and 2 faces per vertex. So the face limit stops it at about 25,000 vertices. Half of the vertex budget and a quarter of the edge and loop budget stay unused. A curved surface cannot be made of quads in this model, because faces must be planar. So terrain and curved surfaces are always triangles.

Cost at the limit (CPU time of one process step, constructed models). The "old" column is the first report.

```text
Model at the limit  Check on each command  Full check  Saved text  Load    Old: check / full
------------------  ---------------------  ----------  ----------  ------  -----------------
grid 222            0.11 s                 0.76 s      7.2 MB      0.20 s  0.13 / 0.50 s
terrain 158         0.09 s                 0.55 s      9.1 MB      0.16 s  0.10 / 0.38 s
sphere 112          0.08 s                 0.55 s      9.4 MB      0.16 s  0.10 / 0.38 s
spiky sphere 112    0.09 s                 3.6 s       9.4 MB      0.16 s  0.10 / 1.6 s
6,250 cubes         0.09 s                 0.40 s      6.0 MB              0.10 / 0.33 s
```

"Load" here is the load of a whole document with the model (parse and check). The first report measured the parse of the model alone, so the two load columns do not compare. The full check was slower than in the first report. Load does not explain it, because this run had less load. The full check is a tool for reports and tests. It is not part of each command, so the limits do not depend on it.

### A2. Edges per edit, after the index

The first report found that `add_edges` cost `c x k x (E + k/2)`: every new segment was compared with all edges. The spatial index (a tree of boxes, built once per edit) removed this. The table is the CPU time of one `edit_surface` command through the editor. "Old" is the first report.

```text
Edges in the edit  Empty model, grid patch: old / now  80k-edge model, same plane: old / now  80k, other plane: old / now
-----------------  ----------------------------------  -------------------------------------  ---------------------------
about 1,000        0.05 / 0.011 s                      10.1 / 0.21 s                          12.9 / 0.18 s
about 2,000        0.26 / 0.026 s                      25.7 / 0.24 s
about 5,000        1.55 / 0.076 s                      62.5 / 0.33 s                          65.6 / 0.25 s
about 10,000       6.45 / 0.18 s                       124 / 0.49 s                           141 / 0.37 s
```

More cases, now only:

```text
Case                                                    CPU time
------------------------------------------------------  -------------------------------------------------------------------
10,000 edges as triangles, empty model                  0.40 s
10,000 edges as triangles, 80k-edge model               0.75 s
10,000 separate segments, empty model                   0.05 s
1,000 triangle edges, 80k-edge model                    0.25 s
1,000 segments, 80k-edge model                          0.16 s
20,000 / 50,000 edges, empty model, in edits of 10,000  0.41 / 1.46 s
20,000 or 50,000 edges in one edit                      refused at once (0.2 ms to 10 ms): at most 10,000 edges in one edit
Erase 1,000 to 50,000 edges, 80k model                  0.13 to 0.16 s
Erase 50,000 edges, empty model                         0.12 s
```

The fixed cost of every command on a model of 80,000 edges is **0.125 s** now. This is an edit that changes nothing. It was 0.155 s. This cost is the copy of the model and the check of the result. The edit itself adds 0.03 s for 1 edge and about 0.1 s for 1,000 edges. The peak heap of an edit on the 80k model is 57 to 91 MB. The model is 14 MB. This is not a limit.

What the index does for the limit of 10,000 edges per edit: an edit of that size now takes 0.2 to 0.75 s. In the first report it took 6 to 190 s. So a person now waits a short time in all cases. This was not true before. The limit stays a safety guard.

### A3. Raised surface limits: models above the current limits

The question is whether to raise faces to 100,000, edges to 150,000 and loop vertices to 300,000. The vertex limit of 50,000 is also near: a triangulated terrain with 99,000 faces has 50,176 vertices. The models below were built with `from_parts`. The limits were multiplied by 3, so all checks ran in full. Each number is the CPU time of a median of 5 runs. The sphere rows of stage `big2` use 3 runs.

```text
Model                             V / E / F           Check per command  Full check  Saved text  Load of the document  Add 1 edge  Add 1,000 edges     Intersect Faces
--------------------------------  ------------------  -----------------  ----------  ----------  --------------------  ----------  ------------------  ---------------
terrain 158 (now: at limit)       25k / 75k / 49.9k   0.09 s             0.55 s      9.1 MB      0.16 s                0.20 s      0.16 s              0.75 s
terrain 194                       38k / 113k / 75k    0.13 s             0.90 s      13.8 MB     0.26 s                0.31 s      0.25 s              1.1 s
terrain 223                       50k / 150k / 99.5k  0.18 s             1.26 s      18.3 MB     0.34 s                0.41 s      0.33 s              1.5 s
sphere 137                        37k / 112k / 74.5k  0.14 s             0.91 s      14.2 MB     0.25 s                0.30 s      0.24 s              1.2 s
sphere 158                        50k / 149k / 99.2k  0.17 s             1.27 s      19.0 MB     0.34 s                0.41 s      0.32 s              1.6 s
sphere 194 (150k faces)           75k / 225k / 150k   0.27 s             2.1 s       28.8 MB     0.51 s                0.63 s      0.49 s              2.5 s
quad grid 222 (now: at limit)     50k / 99k / 49k     0.11 s             0.76 s      7.2 MB      0.20 s                0.26 s      refused (vertices)  0.40 s
quad grid 273                     75k / 150k / 74.5k  0.17 s             1.24 s      10.9 MB     0.30 s                0.40 s      0.35 s              0.62 s
quad grid 316                     100k / 200k / 100k  0.24 s             1.77 s      14.7 MB     0.41 s                0.54 s      0.47 s              0.84 s
spiky sphere 112 (now: at limit)  25k / 75k / 49.7k   0.09 s             3.6 s       9.4 MB      0.16 s                0.20 s      0.16 s              5.7 s
spiky sphere 137                  37k / 112k / 74.5k  0.13 s             7.8 s       14.3 MB     0.25 s                0.30 s      0.24 s              12.5 s
spiky sphere 158                  50k / 149k / 99.2k  0.18 s             13.9 s      19.1 MB     0.34 s                0.44 s      0.34 s              22.6 s
spiky sphere 194 (150k faces)     75k / 225k / 150k   0.27 s             31.7 s      28.9 MB     0.51 s                0.65 s      0.51 s              51.6 s
```

"Add 1 edge" is the fixed cost of a command. A move or another small edit costs the same. Push/Pull of a face took 0.27 s at the limit and 0.55 s on terrain 223. On the spheres the first face (the pole cap) cannot be pushed, because the push cuts through other faces. That is a result of the geometry and not of the size, and it happens at the current limit too.

What the numbers say:

- **Nothing breaks.** Every check, every edit and the load pass at 100,000 faces and at 150,000 faces. The edit log, the undo step and the check work as at 50,000.
- **The cost is linear.** Doubling the faces doubles the cost of a command (0.2 s to 0.4 s). The saved text is 18 to 19 MB at 100,000 triangles and 29 MB at 150,000.
- **The collaboration limit is the first real wall.** One value in collaboration is at most 32 MiB. The whole model is one value (section C). At 100,000 triangles the value is 18.5 MB with the log. At 150,000 it is 29 MB, so 3 MB remain. So 150,000 faces is the most that fits, and 100,000 leaves a factor of 1.7.
- **Pathological meshes are not linear.** A spiky mesh has long thin spikes that cross many boxes. On it the full check and Intersect Faces grow faster than the face count. They take 3.6 s and 5.7 s at 50,000 faces. They take 13.9 s and 22.6 s at 100,000. The check on each command stays linear (0.18 s at 100,000). Intersect Faces on a model of this kind is a deliberate action.
- **Memory is small.** One command on terrain 223 peaks at 126 MB of heap. The model itself is 31 MB.
- The raised values fit the triangulated shapes. Terrain 223 has 150k edges and 298k loop vertices at 99.5k faces. A quad grid needs more, because it has 2 edges and 4 loop vertices per face. A quad grid of 100,000 faces needs 200,000 edges and 400,000 loop vertices. It is still refused.

### A4. Edit log and collaboration size

See section C. The log adds at most 0.5 MB to a value, as in the first report.

## B. Groups and components

### B1. The walk over a definition: before and after

A chain of definitions: each level holds 8 instances of the level below. The bottom is an empty group. The chain has no model. The number of paths grows by 8 for each level. The old walk followed every path. `definition_depth` and `leaves` were measured (CPU time, median of 3 runs). The old code was not run for 12 levels. By extrapolation (64 times the time of 10 levels) `definition_depth` takes about 14 minutes. `leaves` takes about 20 minutes.

```text
Levels  Old `definition_depth`  Old `leaves`            Now: depth, leaves, findings, 3D scene, load (each)  Now: result of `leaves`
------  ----------------------  ----------------------  ---------------------------------------------------  -----------------------------
6       3.4 ms                  5.3 ms                  under 0.1 ms to 13 ms                                ok (262,144 parts visited)
8       0.16 s                  0.28 s                  under 0.1 ms                                         refused, names the definition
10      12.9 s                  19.2 s                  under 0.1 ms                                         refused
12      about 14 min (not run)  about 20 min (not run)  under 0.1 ms                                         refused
```

The new code finds the size of each definition once (models, visited parts and depth). It walks the paths only after the size is within the budget. A command that makes such a chain is refused at the seventh level in 0.1 ms: "component 'D7' is over a limit: has more than 1,000,000 parts, groups and instances ...". Loading a file with the chain takes 0.3 ms and adds a note for it.

### B2. Total of models over all instances

Each instance is a placed copy of the geometry in the 3D scene. The inference scene (the index for snapping) holds its own copy too. The first report estimated 4.7 KB per model. The new measurement, definition of 10 cube parts, instances of it:

```text
Models in all               Mesh: time  Mesh: heap  Mesh: resident memory  Inference scene: time  Inference scene: heap kept  Process maximum for the scene run
--------------------------  ----------  ----------  ---------------------  ---------------------  --------------------------  ---------------------------------
200,000 (20,000 instances)  0.91 s      510 MB      175 MB                 2.3 s                  578 MB                      1.5 GB
250,000 (25,000 instances)  1.14 s      542 MB      238 MB                 2.9 s                  616 MB                      1.9 GB
```

The mesh costs 2.2 KB and the inference scene 2.5 KB per model. Together that is 4.7 KB, which agrees with the first report. So 250,000 models need about 1.2 GB for both. Instances on hidden layers count too, because they still use memory. The process maximum of the scene run is higher. The measurement builds the scene three times to test the cache. The inference itself stays fast (10 microseconds per move, 1 ms for the first).

### B3. Size of a definition

```text
Definition                   Edges   Saved text  Counting the text  Add (`add_resource`)  Open for edit  Close
---------------------------  ------  ----------  -----------------  --------------------  -------------  ------
8,333 cubes                  99,996  11.4 MB     20 ms              95 ms                 130 ms         191 ms
one part with a 16 MiB name  0       16.0 MB     7 ms               17 ms                 59 ms          65 ms
```

At the limits, every operation takes under 0.2 s. The size check adds 20 ms to the add, the change and the close of a definition. The count of the text is done without making the text.

### B4. Explode

An Explode welds all parts of a group or instance into one surface model. The old code copied the model and rebuilt its index for each part. The new code (`Absorber`) keeps one model and its index while it takes in the parts. It finds the faces for their attributes through the index. The result is the same to the bit: a test compares it with the old steps (`absorb_reference`) on random models with attributes, mirrored and turned parts. CPU time of the Explode command of one instance, median of 3 runs. The old code ran once at 10,000 squares and for cubes above 1,000:

```text
Parts                                          Old                                 Now
---------------------------------------------  ----------------------------------  ------------------------------------
1,000 squares (1,000 faces)                    1.39 s                              0.22 s
5,000 squares                                  37.7 s                              5.3 s
10,000 squares (10,000 faces)                  160 s                               21.7 s
1,000 cubes (6,000 faces)                      5.3 s                               0.79 s
3,333 cubes (19,998 faces)                     62.3 s                              7.0 s
5,000 cubes (30,000 faces)                     144 s                               refused in 10 ms (over 20,000 faces)
10,000 cubes (60,000 faces)                    771 s, then refused (first report)  refused in 20 ms
two parts of 10,000 faces each (20,000 faces)                                      0.65 s
```

Explode is 6 to 8 times faster. It is still not linear: ten times more parts take 100 times longer. Each part still makes the solver look at all faces in the plane. The cost depends on the number of parts and not on the number of faces: two big parts with 20,000 faces take 0.65 s. A real fix needs the solver to remember the faces of each plane between edits. That is a change in the core of MOD-06 and not done here.

The cost guard refuses an Explode that welds more than 20,000 faces (`MAX_EXPLODE_FACES`). It refuses before any work. With 10,000 parts at most, the worst accepted case in these measurements is 10,000 squares at 22 s.

### B5. Other operations on a definition of 10,000 parts

The first report measured these operations: `leaves`, `mesh::build`, save, load, `EditComponent`, `CloseComponent`, `MakeUnique` and the undo of each. They took 1 to 360 ms. The new code does not change them, except for the size check of 20 ms on a definition. A definition of 10,000 cubes has 120,000 edges and the size guard now refuses it. So the old test "close a 10,000-cube definition" ends with the message of the size guard.

### B6. Nesting

Unchanged. 3 levels of 27 instances (19,683 models) pass and 3 levels of 28 are refused. A chain with 2 instances per level reaches the model limit before the depth limit. Only a chain with 1 instance per level reaches 16 levels.

## C. Collaboration

One surface model is one value in collaboration, with its edit log. A value holds at most 32 MiB. A transaction holds at most 20,000 operations and 48 MiB.

```text
Command                                              Operations  Update size     Note
---------------------------------------------------  ----------  --------------  -------------------------------------------------------------------------------------
Edit of a model, grid 215 (now)                      1           6.8 MB          the whole shape and the log
Edit of a model, terrain 150 (now)                   1           8.3 MB
Make Group of 4 faces, grid 222 / terrain 158 (now)  5           7.2 / 9.1 MB
Edit of a model, terrain 223, 99.5k faces (raised)   1           18.5 MB         a new client gets the same size as a snapshot
Edit of a model, grid 316, 100k faces (raised)       1           14.9 MB
Make Group, terrain 223 / grid 316 (raised)          5           18.3 / 14.7 MB
Close the edit of 8,333 cubes (99,996 edges)         8,336       12.1 MB         one value of 11.4 MB for the definition, 2 transactions
Close the edit of 10,000 squares                     10,003      6.4 MB          3 transactions
Open the edit of 8,333 cubes                         16,669      14.1 MB         4 transactions
Open the edit of 10,000 squares                      20,003      8.8 MB          refused as one transaction (20,000 operations), 5 after the split of `docsync::split`
```

The largest value after the size guard is the definition: 16 MiB of text at most, which is half of the limit of 32 MiB. The largest value of a model at the current limits is 9.6 MB. At the raised values it is 19 MB. So both stay under the limit. The guard on the definition closes the gap that the first report found: before, each of 10,000 parts held a model of 7 MB.

## Recommendations

"Safety guard" means that the number has no measured cause. It is a protection, and the user is free to choose the value.

- `MAX_VERTICES` (55,000, raised from 50,000 on 2026-10-22): safety guard. A terrain with 99,458 faces has 50,176 vertices, so 55,000 leaves room. At 100k vertices the cost is still linear.
- `MAX_EDGES` (150,000, raised from 100,000 on 2026-10-22): safety guard. At 150k edges a command costs 0.18 s and the text is 18 MB.
- `MAX_FACES` (100,000, raised from 50,000 on 2026-10-22, decided by the user): triangulated meshes can use 50,000 vertices now. At 100k faces the check on each command takes 0.18 s (now 0.09 s). The full check takes 1.3 s. The text is 18 to 19 MB and the load takes 0.34 s. A command takes 0.4 s. The collaboration value is 18.5 MB of 32 MiB. Nothing breaks. Do not go above about 150,000, because the value reaches 29 MB there.
- `MAX_LOOP_VERTICES` (300,000, raised from 200,000 on 2026-10-22): safety guard. Triangles need 3 per face, so 300k fits 100k faces. Quads still stop at 75,000 faces.
- `MAX_EDIT_EDGES` (10,000): keep. Safety guard. It is now also a time bound. An edit of 10,000 edges takes 0.2 to 0.75 s on any model up to 80,000 edges. It took 6 to 190 s before the index.
- `MAX_PARTS` (10,000): keep. Safety guard. Every operation except Explode takes under 0.3 s at 10,000 parts.
- `MAX_LEAVES` (20,000): keep. Safety guard. A walk takes 6 ms and a refusal takes 1 ms. A higher value is possible, but the total of models binds first.
- `MAX_VISITED` (new, 1,000,000): keep. Pure safety guard against a hang. A walk of 262,144 parts takes 4 ms. A refusal takes 0.1 ms. The number has no other cause.
- `MAX_TOTAL_MODELS` (new, 250,000): keep. The memory measurement backs it. 250,000 models need 0.54 GB for the mesh and 0.62 GB for the inference scene. That is 1.2 GB in all.
- `MAX_DEFINITION_EDGES` (new, 100,000): keep. It equals the limit of one model. A definition of 99,996 edges adds, closes and opens in under 0.2 s. Its text is 11.4 MB.
- `MAX_DEFINITION_BYTES` (new, 16 MiB): keep. It is half of the limit of one collaboration value. Add, edit and close of 16 MiB take under 0.07 s. A definition with few edges can still have a lot of text, such as names and logs. The edge limit does not bound that text.
- `MAX_EXPLODE_FACES` (new, 20,000): keep. Cost guard. The cost follows the number of parts. 3,333 cubes (20k faces) take 7 s. 10,000 squares take 22 s. Two parts with 20k faces together take 0.65 s.
- `MAX_EXPLODE_PARTS` (new, 2,000, decided by the user on 2026-10-22): cost guard. It refuses before any work. 1,000 squares take 0.22 s and 5,000 take 5.3 s, so 2,000 parts stay near one second.
- `MAX_COMPONENT_DEPTH` (16): keep. Safety guard. The depth costs nothing.
- `MAX_GROUP_DEPTH` (32): keep. Safety guard. 32 nested groups cost under 0.1 s.
- Instances (no limit): keep. The total of models is the bound. An instance costs 0.6 to 0.9 KB. The file limit of 32 MiB is reached at about 110,000 instances.

## Open points for the user

1. Raise faces to 100,000, edges to 150,000, loop vertices to 300,000 and vertices to about 52,000? The measurements say it works. It doubles the time per command.
2. Explode still grows faster than the number of parts. Is 22 s for 10,000 squares acceptable? If not, the guard can count parts too.
3. A quad grid cannot use the raised values. It needs 200,000 edges for 100,000 faces.
4. Intersect Faces and the full check grow faster than linear on spiky meshes. Do they need their own limit?

## Reproduce

```
export CARGO_TARGET_DIR=<dir> CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export RUSTFLAGS="--cfg kadakas_measure"   # use a target directory of its own for these builds
cargo test --release -p kadakas-core --test limits_measure --no-run
cargo test --release -p kadakas-ui-core --test limits_measure --no-run
cargo test --release -p kadakas-props --test limits_measure --no-run
# BASE: the same test built from the code before the guards (commit 114a7d50), for the old numbers
CORE=... UI=... PROPS=... BASE=... OUT=/tmp/lm-out2 crates/kadakas-core/tests/limits_campaign.sh big   # and big2 guards explode collab2 collab3 a1 a2
```

Single scenario: `LM_SCALE=3 python3 crates/kadakas-core/tests/limits_measure.py BINARY 5 big "terrain 223"`. A scenario test run without `LM` skips itself, so `cargo test -- --ignored` in CI is safe.

## Guides (MOD-05)

A document holds at most 10,000 guides (`kadakas_core::guide::MAX_GUIDES`). A command that adds more is refused and changes nothing. The limit is measured now (release build, Apple M-series, `perf_10000_guides_infer_build_and_draw_within_a_frame`).

The scene had 10,000 guides over a model of 51,520 edges. There were 4,858 lines without ends, 2,500 finite lines and 2,500 points.

| What | Time |
|---|---|
| Scene of the guides for a new revision | 1.7 ms (the model scene: 46.5 ms) |
| One pointer move, overview of the whole model | p50 0.45 ms, p99 0.54 ms, max 1.7 ms |
| One pointer move, close view | p50 0.37 ms, p99 0.62 ms, max 0.8 ms |
| Clip of all 10,000 guides to the view, for drawing | 0.4 ms |

A pointer move costs about 0.4 ms more than without guides. Every line without ends is tested one by one. The test fails above 4 ms at p99, a quarter of a frame.

Hidden Geometry (`perf_ghosts_of_a_large_hidden_model`): a model of 25,600 faces and 51,520 edges, all hidden.

| What | Time |
|---|---|
| Inference scene with the hidden geometry | 79 ms for a new revision |
| Ghosts for a new revision (51,520 edges, 25,600 lines across faces) | 22 ms |
| The same revision again | under 0.001 ms |
| Drawing the ghosts in a frame (shapes and tessellation, 19,280 shapes) | 4.2 ms |

The Tags panel makes its rows once for each revision. Guides are saved as plain objects. One guide is about 80 bytes of text.

## Solid tools (MOD-09)

The solid tools add no limit constant. A cut uses the edit limit of 10,000 new edges (`MAX_EDIT_EDGES`). The result is held to the four count limits of the model. An operation that would pass one is refused and changes nothing. A test builds a bar with 2,600 strips on each of two walls and a slab through it. The cut needs more than 10,000 new edges, and the edit is refused.

Measured once (release build, Apple M1 Pro, one run each, load average about 5 from other work, `perf_two_big_balls` in `surface/tests_solid.rs`, run with `cargo test --release -- --ignored`). The model has two closed balls of 20,480 triangles each, 40,960 faces in all, that cross in a lens.

| What | Time | Faces after |
|---|---|---|
| Closed-solid check (`solid_report`, includes the full check of the model) | 0.49 s | |
| Union | 2.2 s | 30,833 |
| Subtract | 2.1 s | 24,107 |
| Intersect | 1.8 s | 10,759 |
| Trim | 2.4 s | 44,939 |
| Split | 2.5 s | 52,351 |

The same test checks that `vol(A union B) + vol(A intersect B) = vol(A) + vol(B)` and `vol(A subtract B) + vol(A intersect B) = vol(A)` hold on this model. Dense meshes show a case that small models do not: the pieces next to the cut can be thin slivers that touch the other surface within the plane tolerance. A piece is placed by the place that holds at least 80 percent of its area, so such a sliver does not refuse the operation. This is one measurement, not a campaign. A campaign that finds which step costs most is not done. No limit value changes.
