# JavaScript programs

This guide shows how to run JavaScript inside Kadakas, and how a program makes the unit shapes of a generator pattern.

A program is JavaScript on QuickJS. QuickJS runs **inside the WebAssembly sandbox** of Kadakas (the same host as the plug-ins in [plugins.md](plugins.md)), so a program runs the same way on the desktop, on the server and in the browser. A program has no files, no network and no clock. Its only way out is `kadakas.call(method, params)`. That runs a method of the API, with the same dispatch and the same checks as a JSON-RPC request.

**A document holds no code** (PLAT-15). The program is the text of the call: an agent sends it, a person runs a `.js` file from the Scripts window. A document stores results, never programs. Opening it, drawing it, counting it and exporting it run no code.

Python stays the tool for programs that run next to the app, with files and libraries ([scripting.md](scripting.md)). Use JavaScript when the program must run in the session itself, in one undo step, also in the browser.

## Contents

- [The rule: programs only coordinate](#the-rule-programs-only-coordinate)
- [Run a program](#run-a-program)
- [What a program sees](#what-a-program-sees)
- [What a program may call](#what-a-program-may-call)
- [Typed arrays](#typed-arrays)
- [One undo step, and the read mode](#one-undo-step-and-the-read-mode)
- [Limits](#limits)
- [Generators for unit patterns](#generators-for-unit-patterns)
- [The Scripts window](#the-scripts-window)
- [Build the engine](#build-the-engine)
- [What is not done yet](#what-is-not-done-yet)

## The rule: programs only coordinate

Every heavy operation (geometry, Boolean operations, offsets, clipping, pattern layout, meshing, analysis, rendering, import and export) is a native Kadakas call that works on many items in one call. A program decides what to call and with what. If a program needs a loop over many items to do geometry, a native call is missing. Add the call to the API; do not write the loop in the program.

A call costs about 4 microseconds more than the same call from Rust in a release build (see [the measurement](#limits)). A loop of 10,000 cheap calls is fine. A loop of 10,000 calls that each move one point is the sign of a missing call that moves many points.

## Run a program

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

MCP clients find the method in the tool `scripting` (marked open world: the methods a program calls may reach files and the network as far as they could directly). `script.js_info` says how programs run here (limits, the engine, the toolchain that built it).

## What a program sees

- `args`: the value of the parameter `args`.
- `kadakas.call(method, params)`: runs a method of `describe` and returns its result. If the method fails, it throws an `Error` with `code`, `message`, `data` and `method`. A program can catch it.
- `console.log(...)`: writes to the log.
- `Math.random()`: a fixed sequence. `seed` sets it; the same seed gives the same numbers.
- `Date.now()` is always 0, and `new Date()` is the start of 1970. There is no clock.
- Not there: `require`, `fetch`, `process`, files, timers. The code is synchronous: a promise or an `async` function is refused.

The same program gives the same result every time, on every engine (the tests run the engine of the desktop and the engine of the browser side by side).

## What a program may call

A table (`crates/kadakas-api/src/script_js_api/policy.rs`) classifies **every** method of the API by what it can touch: `read`, `edit` (the document only), `ui` (the selection, the view), `files`, `network`, `install` (plug-ins) and `never`. A method that nobody classified is denied, and a test fails when a method has no row.

- A program cannot call `script.*`, `undo`, `redo`, `document.load`, `collab.*`, `events.*`, `feedback.*` and `plugin.*` (the last ones run Python).
- Files, the network and plug-in installs follow the policy of the session: the allowed folders, the allowed addresses. A program has the rights of the agent that sent it, not more.
- In mode `read` a program may call `read` methods only. Any other call is refused **before** it runs, with a policy error (code 4) that the program can catch.
- While a run is open, the session refuses every call that is not one of the run (a conflict), so nothing else becomes part of its undo step.

## Typed arrays

Geometry can be a typed array (`Float64Array`, `Float32Array`, `Int32Array`, `Uint32Array`, `Uint8Array`). It crosses the sandbox as bytes, not as JSON text.

- **Into a call.** A typed array in `params` is written out as numbers before the method sees it, so it works for every method that takes numbers or points. `kadakas.points(array, 2)` marks a flat array as a list of `[x, y]` pairs (stride 2; 3 for `[x, y, z]`). `kadakas.pointsFrom([[0, 0], [10, 0]])` makes one from a list. A marked array becomes `[[x, y], ...]` in the parameters.
- **Out of a call.** `kadakas.call(method, params, {typed: true})` gives long lists of points (8 or more) and long lists of numbers (64 or more) in the answer as `Float64Array` with `.stride`. Short lists stay plain arrays.
- **In the result.** `script.eval` writes a returned typed array out as numbers.

A typed array must not sit inside a list of plain numbers. The size of a call and of its reply, and the count of numbers that typed arrays make, are bounded by the output limit.

## One undo step, and the read mode

All changes of a run are **one undo step**, with the change origin `script`. One `undo` takes back the whole run; one `redo` brings it back. If the program throws, or a limit stops it, the whole run is taken back, and the history and the log of changes show no trace of it. The history limit does not drop steps while a run is open. The collaboration client and `events.poll` see the run as one change.

## Limits

The limits come from the session (`SessionLimits`: `script_js_fuel`, `script_timeout`, `script_js_time`, `script_js_memory_bytes`, `script_js_output_bytes`, `script_js_calls`). The defaults:

| Limit | Desktop and server | Browser |
|---|---|---|
| Fuel (about one unit per WebAssembly instruction; a JavaScript bytecode costs about 100) | 10,000,000,000 | 400,000,000 |
| Time for each stretch of the program between two calls | 5 s (`script_timeout`) | none (fuel only) |
| Time for the whole run, with the calls | 120 s (`script_js_time`) | measured at the calls (`Date.now()`) |
| Memory of the engine | 128 MiB | 128 MiB |
| Size of the answer, and of each call and reply | 16 MiB | 16 MiB |
| Calls into the API | 1,000,000 | 1,000,000 |

The strict limits for free accounts are smaller and never above the browser limits (`SessionLimits::strict`). On the desktop the whole-run time is checked at every call **and** at every tick of the clock of the engine (about every 50 ms), so a program that makes no calls stops too; a run that lasts "5 s" only when it makes no calls is bounded by 5 s per stretch and 120 s in all. The browser cannot interrupt a program by time (a page has no worker to do it), so fuel is the bound there, as for plug-ins. Every call from a program goes through `Session::handle`, so the limits of the session (objects, memory, time of a call) apply to each call too. The memory of the engine inside the module is three quarters of the limit; the rest is for the C library and the frames, which are counted by the memory cap of the module.

**Cost of a call.** Measured on an Apple M-series computer (`measure_call_overhead` in `crates/kadakas-api/src/script_js_api/tests.rs`, 10,000 calls each). Release build: a call of `selection.get` costs 1.1 microseconds from Rust and 5.4 microseconds from a program; `document.summary` 3.7 and 19 microseconds. Debug build: 6 and 23, 16 and 59. An empty run costs 1.3 ms. The first run in a process compiles the module (1.1 s release, 1.5 s debug).

## Generators for unit patterns

A pattern of the kind `generator` (PAT-09) lays unit shapes that a generator made. Kadakas does everything after that, as for the other kinds: clipping to the outline and the holes (also of each zone and course), whole and cut units, counts per unit type, `unit_pattern.check`, reports, `to_objects` and the DXF export.

The pattern **stores the last result** of the generator: the shapes, a `label` (who made them) and the `params` (what they were made with). So a document draws, counts and exports a generator pattern **without running any code**. Today an agent makes the result: it runs a program with `script.eval`, which computes the shapes, and writes them where they belong, in the same run (one undo step). An installed package will make results later (PLAT-15); then the document will name the package, its version and hash, and a person or an agent will make the result again when the parameters change. Regeneration is never automatic.

The shapes, **not clipped**, in drawing coordinates:

```text
{ types: ["name", ...],                                     // the unit types
  rects: { kind: [type index per unit], data: [cx, cy, length, width, angle (degrees), ...] },
  polygons: { kind: [...], start: [n + 1 entries], points: [x, y, ...] } }
```

Like the cells of a bond or a module, the shapes are the outlines **at the centers of the joints**; the gap moves each one inward by half a gap. The full area of a unit type is the area of its largest cell, so a cut unit has a share of it. Typed arrays in the call are written out as numbers, so a program passes the typed arrays of its result as they are. At most `limits.pattern_units` shapes.

In a call: `unit_pattern.layout {region, pattern: {kind: "generator", label, params, types, rects, polygons, gap}, origin}`. In a hardscape style: `hardscape_style.create {name, joints: {kind: "generator", spacing: 10, generator: {label, params, units: {types, rects, polygons}}}}`. The result is stored in the style and lays under every hardscape that uses it; each zone and course clips it to its own region. Run the program again and call `hardscape_style.update {id, joints}` to replace the result.

The two examples below return the shapes when they get `args` with `region.box` and `params`. Wrap a generator in a function and pass its result on:

```js
const generate = (args) => { /* the body of a generator */ };
const result = generate(args);
kadakas.call("hardscape_style.update", {id: args.style, joints: {kind: "generator", spacing: 10,
  generator: {label: "ashlar", params: args.params,
    units: {types: result.types, rects: result.rects}}}});
```

### Example: random ashlar

Courses of equal height; in each course, units of three lengths in a random order. The file is `docs/scripts/random-ashlar.js`.

```js
// Random ashlar: courses of equal height, in each course units of three lengths in a random
// order. A generator program for a pattern of the kind "generator" (PAT-09): it makes the unit
// shapes. Run it with script.eval (see docs/scripting-javascript.md), then write what it
// returns into a pattern; the document stores the result and runs no code.
//
// args.region.box = [x0, y0, x1, y1]: the area to cover (mm), for example the box of a hardscape.
// args.params = { course: 200,              height of a course (mm)
//                 lengths: [300, 450, 600], the three unit lengths (mm)
//                 seed: 1 }                 the same seed gives the same wall
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
// cobbles round a fountain. A generator program for a pattern of the kind "generator" (PAT-09):
// run it with script.eval (see docs/scripting-javascript.md), then write what it returns into a
// pattern; the document stores the result and runs no code.
//
// args.region.box = [x0, y0, x1, y1]: the area to cover (mm).
// args.origin = [x, y] and args.angle (degrees): where the fan is and where its first joint points.
// args.params = { ring: 500,   width of a ring (mm)
//                 unit: 300,   about how long a wedge is along the middle of its ring (mm)
//                 count: 0,    units in every ring; 0 = choose from `unit` for each ring
//                 step: 5 }    degrees of an arc piece (arcs are cut into straight pieces)
//
// Two unit types alternate from ring to ring. The shapes are not clipped; Kadakas cuts them
// to the region.
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

Both programs are tested: the fan over a 2 m square gives 24 units (16 whole, 8 cut), the ashlar fills its region and gives the same wall for the same seed, and a small grid gives 15 units (10 whole, 5 cut) (`crates/kadakas-api/src/script_js_api/tests.rs`).

## The Scripts window

File > Scripts lists `.py` and `.js` files of a folder. **Run** on a `.js` file runs it with `script.eval` in the open document: one undo step, the log and the result in the console, an error in red. The engine runs on a **worker thread**; the window stays alive, serves the calls of the program in each frame (about 8 ms at a time) and shows "Script running" with the time, the calls and a **Stop** button. Stop makes the next call fail, and the clock of the engine interrupts a program that makes no calls. While the program runs, the drawing cannot be edited (a modal window), so the edits of the person do not become part of the undo step of the run. One call can take longer than a frame (a native operation does not stop in the middle). A `.py` file starts as a program, as before. The browser has no threads: there the run is done at once, within its fuel.

## Build the engine

The engine is QuickJS (quickjs-ng 0.17.0, vendored) built to WebAssembly by `crates/kadakas-script-js/build.rs` with a pinned wasi-sdk. The cargo feature `script-js` of `kadakas-api`, `kadakas-cli` and `kadakas-app` turns it on (default). How to install the toolchain is in [dev-guide.md](dev-guide.md#the-javascript-engine-wasi-sdk). Without the feature, `script.eval` says that the engine is not built in; a stored generator result is still laid.

## What is not done yet

- A package registry for generators (PLAT-15): a generator takes its code from an installed package, and the document names the package. For now an agent makes the results.
- The result of a generator is stored inside the pattern as numbers (JSON). A large result will move to a blob of the file (`blobs/`) with the registry.
- The browser path is tested on the desktop with the engine of the browser (a rewrite of the module, run on a plain wasmtime); the glue to the `WebAssembly` API of a page compiles but has not run in a browser. The page must call `ScriptEngine::precompile()` once (the app does, at start).
- The joint editor of the app shows the generator kind but does not edit it.
- Script-based plug-in objects (SCR-04) still use Python. They can move to this engine later.
