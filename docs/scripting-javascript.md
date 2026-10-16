# JavaScript scripts

This guide shows how to run JavaScript inside Kadakas, and how to use a script as a generator of unit patterns.

A script is JavaScript on QuickJS. QuickJS runs **inside the WebAssembly sandbox** of Kadakas (the same host as the plug-ins in [plugins.md](plugins.md)), so a script runs the same way on the desktop, on the server and in the browser. A script has no files, no network and no clock. Its only way out is `kadakas.call(method, params)`. That runs a method of the API, with the same dispatch and the same checks as a JSON-RPC request. So a script can do no more than the API allows, and it needs no extra consent.

Python stays the tool for programs that run next to the app, with files and libraries ([scripting.md](scripting.md)). Use JavaScript when the script must run in the session itself, in one undo step, also in the browser.

## Contents

- [The rule: scripts only coordinate](#the-rule-scripts-only-coordinate)
- [Run a script](#run-a-script)
- [What a script sees](#what-a-script-sees)
- [Typed arrays](#typed-arrays)
- [One undo step, and the read mode](#one-undo-step-and-the-read-mode)
- [Limits](#limits)
- [Script resources and trust](#script-resources-and-trust)
- [Generators for unit patterns](#generators-for-unit-patterns)
- [The Scripts palette](#the-scripts-palette)
- [Build the engine](#build-the-engine)
- [What is not done yet](#what-is-not-done-yet)

## The rule: scripts only coordinate

Every heavy operation (geometry, Boolean operations, offsets, clipping, pattern layout, meshing, analysis, rendering, import and export) is a native Kadakas call that works on many items in one call. A script decides what to call and with what. If a script needs a loop over many items to do geometry, a native call is missing. Add the call to the API; do not write the loop in the script.

A call costs about 17 microseconds more than the same call from Rust (see [the measurement](#limits)). A loop of 10,000 cheap calls is fine. A loop of 10,000 calls that each move one point is the sign of a missing call that moves many points.

## Run a script

`script.eval {code, mode?, args?, seed?}` runs the code as the body of a function and returns its result:

```json
{"method": "script.eval", "params": {"code": "const s = kadakas.call('document.summary', {}); return s.layers.length;", "mode": "read"}}
```

The answer is `{result, log, calls, changed, undo_steps, elapsed_ms}`. `log` holds what `console.log` wrote (at most 64 KiB). In Python:

```python
doc = kadakas.connect()
r = doc.script_eval("""
  const ids = [];
  for (let i = 0; i < args.n; i++)
    ids.push(kadakas.call('execute', {command: {op: 'add_object',
      shape: {kind: 'circle', center: [i * 1000, 0], radius: 300}}}).created[0]);
  return ids;
""", args={"n": 5})
```

MCP clients find the method in the tool `scripting`. `script.js_info` says how scripts run here (limits, the engine, the toolchain that built it).

## What a script sees

- `args`: the value of the parameter `args`.
- `kadakas.call(method, params)`: runs a method of `describe` and returns its result. If the method fails, it throws an `Error` with `code`, `message`, `data` and `method`. A script can catch it.
- `console.log(...)`: writes to the log.
- `Math.random()`: a fixed sequence. `seed` sets it; the same seed gives the same numbers.
- `Date.now()` is always 0, and `new Date()` is the start of 1970. There is no clock.
- Not there: `require`, `fetch`, `process`, files, timers.

A script cannot call `script.eval`, `script.run`, `undo`, `redo`, `document.load`, `collab.*` and `events.*`. In mode `read` it cannot call `export.*`, `import.*`, `file.*`, `gis.*`, `update.*` and other methods that write files or use the network.

The same script gives the same result every time. A test for this is in the test suite of the crate.

## Typed arrays

Geometry can be a typed array (`Float64Array`, `Float32Array`, `Int32Array`, `Uint32Array`, `Uint8Array`). It crosses the sandbox as bytes, not as JSON text.

- **Into a call.** A typed array in `params` is written out as numbers before the method sees it, so it works for every method that takes numbers or points. `kadakas.points(array, 2)` marks a flat array as a list of `[x, y]` pairs (stride 2; 3 for `[x, y, z]`). `kadakas.pointsFrom([[0, 0], [10, 0]])` makes one from a list. A marked array becomes `[[x, y], ...]` in the parameters.
- **Out of a call.** `kadakas.call(method, params, {typed: true})` gives long lists of points (8 or more) and long lists of numbers (64 or more) in the answer as `Float64Array` with `.stride`. Short lists stay plain arrays.
- **In `args` and in the result.** A generator gets its region as typed arrays and returns the units as typed arrays. `script.eval` writes a returned typed array out as numbers.

A typed array must not sit inside a list of plain numbers.

## One undo step, and the read mode

All changes of a run are **one undo step**, with the change origin `script`. One `undo` takes back the whole run; one `redo` brings it back. If the script throws, or a limit stops it, the whole run is taken back, and the history shows no trace of it.

`mode: "read"` is for checks and reports. A call that changes the document fails with a policy error (code 4), and the change is taken back at once. The script can catch the error. A call that writes a file or uses the network is refused before it runs.

## Limits

The limits come from the session (`SessionLimits`: `script_js_fuel`, `script_timeout`, `script_js_time`, `script_js_memory_bytes`, `script_js_output_bytes`, `script_js_calls`). The defaults:

| Limit | Desktop and server | Browser |
|---|---|---|
| Fuel (about one unit per WebAssembly instruction; a JavaScript bytecode costs about 100) | 10,000,000,000 | 400,000,000 |
| Time for each stretch of the script between two calls | 5 s (`script_timeout`) | none (fuel only) |
| Time for the whole run, with the calls | 120 s (`script_js_time`) | none (the page has no clock) |
| Memory of the engine | 128 MiB | 128 MiB |
| Size of the answer (value, log, typed arrays) | 16 MiB | 16 MiB |
| Calls into the API | 1,000,000 | 1,000,000 |

The strict limits for free accounts are smaller (`SessionLimits::strict`). The native engine counts fuel exactly. The browser engine rewrites the module so that it counts its own fuel; it cannot stop a script by time (a page has no worker to interrupt), so fuel is the bound there, as for plug-ins. Every call from a script goes through `Session::handle`, so the limits of the session (objects, memory, time of a call) apply to each call too.

**Cost of a call.** Measured in a debug build on an Apple M-series computer (`measure_call_overhead` in `crates/kadakas-api/src/script_js_api/tests.rs`): a call of `selection.get` costs 6 microseconds from Rust and 23 microseconds from a script; `document.summary` 16 and 59 microseconds. An empty run costs 1 ms. The first run in a process compiles the module (1.5 s). A round trip uses about 46,000 WebAssembly instructions of fuel.

## Script resources and trust

A script can live in the document as a resource (SCR-03, lite): `script_resource.create {name, code, description?}`, `script_resource.get`, `script_resource.update`, `script_resource.list`; delete it with `resource.delete`. The resource keeps the name, the code and the SHA-256 of the code.

- A script that an agent sends in `script.eval {code}` is the agent's own text and runs without trust, for that call.
- A script from the document runs only after a person trusted its **exact text** (the SHA-256). The person trusts it in the Scripts window (File > Scripts, "Scripts in this document"), in a dialog that shows the whole text. A changed text has a new hash and needs trust again. **No API method adds trust**, and none removes it. When the host lets agents run scripts (`--allow-agent-scripts`), they may run scripts of the document without trust, as for plug-ins.
- `script.eval {script: "Name"}` and a generator pattern both need that trust.

## Generators for unit patterns

A pattern of the kind `generator` (PAT-09) lets a script make the unit shapes. Kadakas does everything after that, as for the other kinds: clipping to the outline and the holes, whole and cut units, counts per unit type, `unit_pattern.check`, reports, `to_objects` and the DXF export.

```json
{"method": "unit_pattern.layout", "params": {
  "region": {"outline": [[0, 0], [6000, 0], [6000, 3000], [0, 3000]]},
  "pattern": {"kind": "generator", "script": "Ashlar", "params": {"seed": 7}, "gap": 10},
  "origin": [0, 0]}}
```

In a hardscape style: `hardscape_style.create {name, joints: {kind: "generator", generator: {script: "Ashlar", params: {...}}, spacing: 10}}`. The origin and the angle of the hardscape, its zones and its courses work as for other patterns; each zone calls the script for its own region.

**Input.** The script gets `args`:

```text
{ params,                 // generator.params
  origin: [x, y],          // where the pattern starts (mm)
  angle,                   // degrees, counterclockwise
  gap,                     // the width of a joint
  max_units,               // the most shapes it may return (the unit limit of the session)
  region: { box: [x0, y0, x1, y1], area,
            outer: Float64Array (stride 2), holes: [Float64Array, ...],      // the first shape
            shapes: [{ outer, holes }, ...] } }                              // all shapes
```

**Output.** The unit shapes, **not clipped**, as typed arrays:

```text
{ types: ["name", ...],                                     // the unit types
  rects: { kind: Uint32Array, data: Float64Array },         // cx, cy, length, width, angle (degrees) per unit
  polygons: { kind: Uint32Array, start: Uint32Array,         // start has n + 1 entries
              points: Float64Array } }                      // x, y pairs of all units
```

Like the cells of a bond or a module, the shapes are the outlines **at the centers of the joints**; the gap moves each one inward by half a gap. The full area of a unit type is the area of its largest cell, so a cut unit has a share of it.

**Rules.** A generator is a pure function of its input: it cannot call the API, it has no clock, and `Math.random` is a fixed sequence. Kadakas keeps the answer for the same input (region, parameters, origin, angle, gap, text of the script), so the pattern is "regenerated" whenever one of them changes, and not otherwise. The result is not stored in the document: the script, the parameters and the object are the editable data, and any client that has the script can make the units again. Bounds: fuel, time, memory and the unit count of the session (`pattern_units`); a script that makes more units fails with the number it made.

### Example: random ashlar

Courses of equal height; in each course, units of three lengths in a random order. The file is `docs/scripts/random-ashlar.js`.

```js
// Random ashlar: courses of equal height, in each course units of three lengths in a random
// order. A generator for a pattern of the kind "generator" (PAT-09).
//
// params: { course: 200,                    height of a course (mm)
//           lengths: [300, 450, 600],       the three unit lengths (mm)
//           seed: 1 }                       the same seed gives the same wall
//
// The shapes are the cells at the centers of the joints, not clipped. Kadakas moves them inward
// by half the gap and cuts them to the region and its holes.
const p = args.params || {};
const course = p.course || 200;
const lengths = p.lengths || [300, 450, 600];
let state = (p.seed === undefined ? 1 : p.seed) >>> 0;

// A small random generator with a seed of our own (xorshift32), so the result does not depend on
// anything but the parameters.
function next() {
  state ^= state << 13; state >>>= 0;
  state ^= state >>> 17;
  state ^= state << 5; state >>>= 0;
  return state / 4294967296;
}

const [x0, y0, x1, y1] = args.region.box;
const rows = Math.ceil((y1 - y0) / course);
const rects = [];
const kinds = [];
for (let r = 0; r < rows; r++) {
  const cy = y0 + (r + 0.5) * course;
  // Start a little to the left of the region by a random part of a unit, so the joints of
  // neighbouring courses do not line up.
  let x = x0 - next() * lengths[0];
  while (x < x1) {
    const k = Math.min(lengths.length - 1, Math.floor(next() * lengths.length));
    const len = lengths[k];
    rects.push(x + len / 2, cy, len, course, 0);
    kinds.push(k);
    x += len;
  }
}
return {
  types: lengths.map((l) => `${l}x${course}`),
  rects: { kind: new Uint32Array(kinds), data: new Float64Array(rects) },
};
```

### Example: radial fan

Rings of wedge-shaped units round the origin of the pattern. The file is `docs/scripts/radial-fan.js`.

```js
// Radial fan: rings of wedge-shaped units around the origin of the pattern, like a fan of
// cobbles round a fountain. A generator for a pattern of the kind "generator" (PAT-09).
//
// params: { ring: 500,       width of a ring (mm)
//           unit: 300,       about how long a wedge is along the middle of its ring (mm)
//           count: 0,        units in every ring; 0 = choose from `unit` for each ring
//           step: 5 }        degrees of an arc piece (arcs are cut into straight pieces)
//
// The fan starts at args.origin and turns with args.angle, so the pattern origin and angle of
// the object set where the fan is and where its first joint points. Two unit types alternate
// from ring to ring. The shapes are not clipped; Kadakas cuts them to the region.
const p = args.params || {};
const ring = p.ring || 500;
const unit = p.unit || 300;
const step = p.step || 5;
const [cx, cy] = args.origin;
const turn = (args.angle * Math.PI) / 180;
const [x0, y0, x1, y1] = args.region.box;
// The fan reaches the farthest corner of the box.
const reach = Math.max(
  Math.hypot(x0 - cx, y0 - cy), Math.hypot(x1 - cx, y0 - cy),
  Math.hypot(x0 - cx, y1 - cy), Math.hypot(x1 - cx, y1 - cy));
const rings = Math.ceil(reach / ring);

const points = [];
const start = [0];
const kinds = [];
for (let r = 0; r < rings; r++) {
  const r0 = r * ring, r1 = (r + 1) * ring;
  const n = p.count > 0 ? p.count : Math.max(3, Math.round((2 * Math.PI * (r0 + r1) / 2) / unit));
  for (let i = 0; i < n; i++) {
    const a0 = turn + (2 * Math.PI * i) / n, a1 = turn + (2 * Math.PI * (i + 1)) / n;
    const pieces = Math.max(1, Math.ceil(((a1 - a0) * 180) / Math.PI / step));
    // Outer arc from a0 to a1, then the inner arc back (a ring that starts at the center has a
    // point there instead).
    for (let k = 0; k <= pieces; k++) {
      const a = a0 + ((a1 - a0) * k) / pieces;
      points.push(cx + r1 * Math.cos(a), cy + r1 * Math.sin(a));
    }
    if (r0 === 0) {
      points.push(cx, cy);
    } else {
      for (let k = pieces; k >= 0; k--) {
        const a = a0 + ((a1 - a0) * k) / pieces;
        points.push(cx + r0 * Math.cos(a), cy + r0 * Math.sin(a));
      }
    }
    start.push(points.length / 2);
    kinds.push(r % 2);
  }
}
return {
  types: ["even ring", "odd ring"],
  polygons: {
    kind: new Uint32Array(kinds),
    start: new Uint32Array(start),
    points: new Float64Array(points),
  },
};
```

Both scripts are tested: the fan over a 2 m square gives 24 units (16 whole, 8 cut), the ashlar fills its region and gives the same wall for the same seed (`crates/kadakas-api/src/script_js_api/tests.rs`).

## The Scripts palette

File > Scripts lists `.py` and `.js` files of a folder. **Run** on a `.js` file runs it with `script.eval` in the open document: one undo step, the log and the result in the console, an error in red. The run uses the UI thread, within the limits of the session. A `.py` file starts as a program, as before.

The window also lists the **scripts in the document** with their state. A script that is not trusted has a Review button (the trust dialog); a trusted one has Run and Stop trusting.

## Build the engine

The engine is QuickJS (quickjs-ng 0.17.0, vendored) built to WebAssembly by `crates/kadakas-script-js/build.rs` with a pinned wasi-sdk. The cargo feature `script-js` of `kadakas-api`, `kadakas-cli` and `kadakas-app` turns it on (default). How to install the toolchain is in [dev-guide.md](dev-guide.md#the-javascript-engine-wasi-sdk). Without the feature, `script.eval` says that the engine is not built in.

## What is not done yet

- The browser path is tested on the desktop with the engine of the browser (a rewrite of the module, run on a plain wasmtime); the glue to the `WebAssembly` API of a page compiles but has not run in a browser. The page must call `ScriptEngine::precompile()` once (the app does, at start).
- A script runs on the thread of the caller. In the app, a long script blocks the window until the limit (5 s per stretch).
- A generator needs its pattern in a hardscape style or in the `pattern` parameter. The joint editor of the app does not edit a generator yet (it shows the kind).
- Read mode refuses a call that changes the document after the call ran (and takes it back); a method that has other effects than on the document (the selection, the view) is allowed.
- Script-based plug-in objects (SCR-04) still use Python. They can move to this engine later.
