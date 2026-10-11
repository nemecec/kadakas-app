# Plug-ins

This guide shows how to write a plug-in for Kadakas and how to use one.

A plug-in is a small WebAssembly program (a `.wasm` file). It makes a parametric object, such as a raised planter. You give it parameters and a place. It gives back shapes. A plug-in can also offer a drawing tool: it answers to clicks and shows a preview.

Plug-ins run in a sandbox. They cannot read files, use the network or read the clock. They run on the desktop, on a server and in the browser. The limits are the same everywhere, with one exception in the browser (see [Browser and server](#browser-and-server)).

Python stays the tool for automation scripts. See [scripting.md](scripting.md). Python plug-in objects still work. If the object must run the same everywhere, use a WebAssembly plug-in. It is also the better choice for objects that you share with other people. They need not trust a Python script.

## Contents

- [How it works](#how-it-works)
- [Use a plug-in](#use-a-plug-in)
- [Use plug-ins in the app](#use-plug-ins-in-the-app)
- [Tools](#tools)
- [Write a plug-in](#write-a-plug-in)
- [The ABI](#the-abi)
- [Safety and limits](#safety-and-limits)
- [Threat model](#threat-model)
- [API reference](#api-reference)
- [Browser and server](#browser-and-server)
- [What is not done yet](#what-is-not-done-yet)

## How it works

1. You install a module: `plugins.install {path}`. Kadakas reads the manifest of the module (name, version, parameters). It runs nothing else.
2. A user trusts the module. Trust is for the exact bytes of the file (the SHA-256 hash). A new build of the plug-in needs new trust.
3. You place an object: `plugins.place`. Kadakas calls the module with the parameters and the place. The module returns shapes.
4. Kadakas stores the shapes as a group in the drawing. The parameters are in the data of the group. Each change is one undo step. The change origin is `script`.

The shapes are normal shapes: lines, rectangles, circles, arcs, polylines and text. Any client draws them, with or without the plug-in. A drawing with plug-in objects opens on a computer that does not have the plug-in. You need the plug-in only to change the parameters.

## Use a plug-in

```python
import kadakas

doc = kadakas.connect()      # to an app or a "kadakas serve" that you started with --allow-agent-scripts
api = doc.api
info = api.plugins_install(path="crates/kadakas-plugin/tests/fixtures/raised_planter.wasm")
planter = api.plugins_place(plugin=info["id"], min=[0, 0], max=[2400, 900],
                            params={"wall": 150, "corners": "Cut"})
api.plugins_set_params(id=planter["id"], params={"height": 600})
```

Or with raw JSON-RPC:

```json
{"method": "plugins.install", "params": {"path": "/home/me/plugins/raised_planter.wasm"}}
{"method": "plugins.place", "params": {"plugin": "org.kadakas.raised-planter", "min": [0, 0], "max": [2400, 900], "params": {"corners": "Cut"}}}
```

Trust works like this:

- A module that nobody trusted does not run. The call fails with the error code `4` (kind `policy`). The message shows the hash.
- The user trusts a module in the app, in the trust dialog (see [Use plug-ins in the app](#use-plug-ins-in-the-app)). The trust list is `trusted_wasm_plugins.json` in the folder `plugins` of the user data folder (see [Keep modules between sessions](#keep-modules-between-sessions)).
- **No API method adds trust, and none removes it.** There is no `plugins.trust`. An agent cannot decide for the user. The trust dialog and the button "Stop trusting" are in the app only, and the function behind them is not an API method. Agents can run plug-ins only when the host allows it: start `kadakas serve`, `kadakas mcp` or the app with `--allow-agent-scripts`. This is the same rule as for Python scripts. `plugins.list` shows `trusted` (a user trusted this exact module) and `runnable` (trusted, or the flag is set).
- A sandboxed module is safer than a Python script, but it can still draw wrong shapes or waste time. That is why the user still approves each module.

## Upgrade a plug-in

Install the new build with `plugins.install`. It replaces the old build with the same id. The objects keep their shapes. `plugins.object` shows `stale: true` for them, and `plugins.install` says how many there are. The new build must be trusted again (it has a new hash). Then call `plugins.reset {plugin}`. This regenerates all objects of the plug-in in one undo step.

When a reset reads the old parameters of an object, it drops the ones that the new schema does not know, and it uses the default for the ones that no longer fit (for example, a value outside the new range). `plugins.set_params` with `null` for a name puts the default back.

A reset replaces only the shapes that the plug-in made. Shapes that you added to the group stay. The class of the shapes stays too. A move of the group is followed, as long as you move the shapes of the plug-in.

If the module is not installed, the objects show their shapes and `plugins.object` says `installed: false`.

## Keep modules between sessions

The app keeps installed modules and the trust list for the next session:

- **Desktop.** Modules are files in `plugins/modules/<sha256>.wasm`, and the trust list is `plugins/trusted_wasm_plugins.json`, both in the user data folder (`~/Library/Application Support/kadakas` on macOS, `%APPDATA%\kadakas` on Windows, `$XDG_DATA_HOME/kadakas` or `~/.local/share/kadakas` elsewhere). The files are private to the user (mode 0600 on Unix) and are written atomically.
- **Browser.** The same data is in the `localStorage` of the page, as text. If the browser gives no storage (a private window, storage switched off) or a write fails (the quota is full), the module still works in this session and the app says that it is not kept. The browser keeps at most 2 MiB of modules in total.
- **Checks on load.** The name of a stored module is its SHA-256. On load the host reads at most 2 MiB per file, computes the hash again and refuses a file that does not match. A changed file is not compiled and not run. It stays on the disk and the app reports it at the start. Then the module goes through the same checks as a new install (size, structure, manifest, no imports). The total of all stored modules is capped (64 MiB on the desktop, 2 MiB in the browser). A module that does not fit is installed for the session but not saved.
- **Listing runs nothing.** Next to each module the app stores its description as `<sha256>.manifest.json`. At start the app lists the plug-ins from these records and does not compile or run any module. A module is compiled, and `describe` runs, the first time it is needed (a review, or a trusted use), and its answer must equal the stored description or the module is refused. A stored module without a description is reported and not loaded.
- **Trust follows the hash.** A stored module that was changed would have another hash, so it cannot be trusted by accident.
- **Tests and servers.** The folder is a parameter of the host (`Registry::open_home`). Only the app turns it on, with the folder of the user. `kadakas serve`, `kadakas mcp` and all tests keep modules in memory. The trust list moved from the config folder to the data folder with this change; trust that an older build wrote must be given again.

## Use plug-ins in the app

- **File > Plug-ins** opens the Plug-ins window. It lists the installed plug-ins with their state (trusted or not), the number of objects that use each one, and the buttons Review, Stop trusting and Remove (Remove is off while objects use the plug-in). **Install plug-in...** picks a `.wasm` file.
- **The trust dialog** opens when you install a module that is not trusted, when you press Review, when you choose a tool of a module that is not trusted, and when you press Review in Object Info. It shows the name, the version, the author (the module says it; the host does not check it), the SHA-256, the size in bytes, what the module can do (make parametric objects, draw with a tool) and the description. The module asks for no permissions, because the host grants none, so the dialog has none to show: it says that the module cannot read your drawing, your files, the network or the clock. The buttons are **Trust** and **Cancel**. Trust is for the exact bytes. A new build has a new hash and the dialog opens again.
- **Object Info** shows a form for a selected plug-in object. A number shows its range (as a hint) and its unit; a length uses the length unit of the person; there are checkboxes, choices, text fields, colors and groups of fields. The form checks what you type when you leave the field or press Enter. A wrong value stays in the field with a message and nothing changes. A right value is one `plugins.set_params` call: one undo step with the change origin `script`. Esc puts the saved value back, and **Default** puts the default of one parameter back. **Run again** makes the shapes again. If the plug-in is not trusted, the form is off and has a Review button. If the last run failed, Object Info shows the error and says that the shapes are from the last good run.
- **The tool set "Plug-ins"** in the Tools palette has one button for each installed plug-in that has a tool.

## Tools

A plug-in that lists the kind `tool` can draw with the mouse. The messages are JSON, as for `reset`, and the same limits apply to each call.

The host sends **events** in drawing coordinates (mm):

| Event | When |
|---|---|
| `{"type": "start"}` | The person chose the tool. |
| `{"type": "move", "at": [x, y]}` | The pointer moved. The app sends it only when the pointer moved by more than 2 pixels. |
| `{"type": "click", "at": [x, y]}` | The person clicked. |
| `{"type": "finish"}` | The person pressed Enter or double-clicked. |

Esc never reaches the module. The host drops the state and the preview.

The request has the event, the **modifiers** (`{"shift": false, "ctrl": false, "alt": false}`; `ctrl` is Cmd on macOS), the **state**, the tool **params** (defaults filled in) and `"units": "mm"`. The answer is `{"status": "ok", "state": ..., "preview": [shapes], "hint": "text", "result": null}` or `{"status": "error", "message": "..."}`.

- **State.** Every call runs in a new instance, so the module has no memory between calls. It puts what it needs in `state` (any JSON, at most 64 KiB) and the host sends it back with the next event. The host does not look inside the state.
- **Preview.** Shapes as in `reset`. The app draws them on the canvas. They never enter the drawing. An empty list is fine.
- **Hint.** A short text for the status bar (at most 200 characters; control characters become spaces). It is text of the plug-in, so it is not translated.
- **Result.** When `result` is set, the run ends. It is `{"type": "shapes", "shapes": [...]}` (plain shapes, at least one) or `{"type": "object", "params": {...}, "geometry": {...}}`. An object result is always an object **of the same module**: the host checks the geometry (the kind and the numbers) and the parameters against the manifest, calls `reset`, and adds the group like `plugins.place`. A module cannot place the object of another module. The module must provide the kind `object` for that.
- **Errors.** `{"status": "error", "message": "..."}` is a refusal by the module, for example a click that is too close. In the app the run goes on and the message shows in the status bar. A limit, a crash or a bad answer ends the run, so a module that fails is not asked again for every mouse move.
- **Manifest.** `"kinds": ["tool"]` (or both) and a `"tool": {"params": [...]}` part. The tool parameters use the schema of object parameters. The app has no editor for them yet and uses the defaults. An agent can pass `params` to `plugins.tool_step`.
- **API.** `plugins.tool_step {plugin, event, modifiers?, state?, params?, commit?}` sends one event. With `commit` (the default) the result goes into the drawing in one undo step with the change origin `script`.

The app asks for trust before the first event. An agent needs the trust of the user too (or the host flag `--allow-agent-scripts`).

The example `plugins/stone-row` is an object (a row of stepping stones along a line) and a tool (click the start, move to see the stones, click the end; Shift ends with plain shapes). It has about 150 lines.

## Write a plug-in

You can use any language that makes WebAssembly. The examples use Rust, with the crate `kadakas-plugin-abi`. It has the manifest and message types, so you do not write JSON by hand.

The example `plugins/raised-planter` is a good start. It has about 120 lines. The example `plugins/stone-row` shows a tool.

### 1. Make the crate

`Cargo.toml`:

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
kadakas-plugin-abi = { path = "../kadakas/crates/kadakas-plugin-abi" }

[workspace]            # keeps the plug-in apart from other workspaces

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

### 2. Describe the plug-in

```rust
use kadakas_plugin_abi::{
    Geometry, Manifest, ObjectSpec, OutShape, ParamSchema, ParamType, ResetRequest, ShapeData,
    ShapeType, export_plugin,
};
use kadakas_plugin_abi::serde_json::json;

fn manifest() -> Manifest {
    Manifest::object(
        "org.example.stone",            // a stable id: lower case, digits, . - _
        "Stone",                        // the name that users see
        "1.0.0",
        ObjectSpec {
            shape_type: ShapeType::Point,   // placed with a point and a rotation
            params: vec![
                ParamSchema::new("size", ParamType::Length)
                    .with_default(json!(400))
                    .with_range(50.0, 2000.0),
            ],
        },
    )
}
```

The shape type says how a user places the object:

| `shape_type` | Geometry the module gets |
|---|---|
| `point` | `{at: [x, y], rotation}` |
| `linear` | `{a: [x, y], b: [x, y]}` |
| `rectangle` | `{min: [x, y], max: [x, y]}` |
| `path` | `{points: [[x, y], ...], closed}` |

Parameter types are `number` (with an optional `unit` text such as `pcs`, shown next to the field), `length` (mm), `text`, `bool`, `choice` (with `options`), `color` (`#rrggbb`) and `record` (a group of fields). `min` and `max` give the range of a number or a length. Kadakas checks every value before it calls your module. Your module always gets all parameters, with defaults filled in.

### 3. Do the reset

```rust
fn make_shapes(request: ResetRequest) -> Result<Vec<OutShape>, String> {
    let Geometry::Point { at, .. } = request.geometry else {
        return Err("a stone needs a point".into());
    };
    let size = request.params["size"].as_f64().unwrap_or(400.0);
    Ok(vec![OutShape::new(ShapeData::Circle { center: at, radius: size / 2.0 })])
}

export_plugin!(describe = manifest, reset = make_shapes);
```

A tool plug-in writes `export_plugin!(describe = manifest, tool = run_tool);`, and a plug-in with both writes `export_plugin!(describe = manifest, reset = make_shapes, tool = run_tool);`. The function that you leave out answers with an error.

Rules for the answer:

- Return at least one shape.
- Units are millimeters. The Y axis points up. Angles are radians.
- Shape kinds: `line`, `rect`, `circle`, `arc`, `polyline` and `text`. A polyline has plain corner points (no curves yet).
- Colors are `[r, g, b, a]`. Leave them out to use the class colors. `color_rgba("#6b4f3a")` turns a color parameter into this form.
- If the module cannot make the object (for example, it is too small), return `Err(message)`. The user sees your message. The old shapes stay.
- Do not use `panic!`. A panic ends the call with a crash message and no detail.
- The exports of the module are called `alloc`, `dealloc`, `describe`, `reset` and `tool`. Do not use these names for your own functions. That is why the example calls its function `make_shapes`.

### 4. Build it

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
# the module is in target/wasm32-unknown-unknown/release/my_plugin.wasm
```

The module has no imports. A module built for WASI (`wasm32-wasip1`) imports system calls, so Kadakas refuses it. Use `wasm32-unknown-unknown`. If your language adds imports, turn them off.

### 5. Test it

Install the module in a session (see [Use a plug-in](#use-a-plug-in)). Use `plugins.preview` to see the shapes. The drawing does not change. For a test in the Kadakas repository, copy the module to `crates/kadakas-plugin/tests/fixtures/`. The tests of `kadakas-plugin` show how to load a module and call `reset` from Rust.

The example module in the repository is rebuilt with `scripts/build-plugin-fixtures.sh`. The tests use the committed `.wasm` file, so they do not need the wasm target.

## The ABI

The ABI (application binary interface) is the contract between the host and the module. This is version `2`. The host refuses a module with another version. Version 2 added the export `tool`, the manifest field `author` and the manifest part `tool`; rebuild a module of version 1 against this crate.

The module has these exports and no imports:

| Export | Meaning |
|---|---|
| `memory` | The linear memory of the module. |
| `alloc(len: u32) -> u32` | Reserve `len` bytes. Return the address. |
| `dealloc(ptr: u32, len: u32)` | Free a block. The Kadakas host does not call it (see below), but the export must exist. |
| `describe() -> u64` | Return the manifest as JSON. |
| `reset(ptr: u32, len: u32) -> u64` | Read the request at `ptr`. Return the answer as JSON. For the kind `object`. |
| `tool(ptr: u32, len: u32) -> u64` | Read the tool request at `ptr`. Return the answer as JSON. For the kind `tool`. |

A `u64` result holds an address in the high 32 bits and a length in the low 32 bits. A result of `0` means "no answer", which is an error.

The host calls `reset` in four steps. It calls `alloc`. It writes the request into the memory. It calls `reset`. Then it reads the answer from the memory. Every call runs in a new instance, so nothing stays in memory between calls. The host drops the instance after the call, so it does not call `dealloc`. Your module needs no global state.

The manifest:

```json
{
  "id": "org.kadakas.raised-planter",
  "name": "Raised planter",
  "version": "1.0.0",
  "abi_version": 2,
  "description": "A raised planter.",
  "author": "The Kadakas project",
  "kinds": ["object"],
  "object": {"shape_type": "rectangle", "params": [{"name": "wall", "type": "length", "default": 150, "min": 50, "max": 500}]},
  "permissions": []
}
```

The request:

```json
{"params": {"wall": 150}, "geometry": {"type": "rectangle", "min": [0, 0], "max": [2400, 900]}, "units": "mm"}
```

The answer is `{"status": "ok", "shapes": [...]}` or `{"status": "error", "message": "..."}`. A shape is `{"kind": "line", "a": [0, 0], "b": [10, 0], "attrs": {"pen": [0, 0, 0, 255], "line_weight": 0.35}}`. The `attrs` are optional.

`kinds` can be `object`, `tool` or both. The kind `object` needs an `object` part, and the kind `tool` needs a `tool` part. The `author` is text for the trust dialog. The host shows it and does not check it.

`permissions` can name `read_document`, `filesystem`, `network` or `clock`. ABI version 2 has no calls from the module to the host. The host grants none. It refuses a module that asks.

Version rules: a change that breaks old modules gets a new ABI version. New optional fields in the JSON do not. The host ignores nothing silently: an unknown shape kind or field in the answer is an error.

## Safety and limits

| Protection | How |
|---|---|
| Valid modules only | Every module is *validated* (not only parsed) against a fixed list of WebAssembly features before any engine sees it. The list: mutable globals, sign extension, saturating float to int, multi-value, bulk memory, reference types, SIMD and tail calls. Everything else is off, in particular exceptions, garbage collected types, threads, memory64, custom page sizes, multiple memories, relaxed SIMD, wide arithmetic and stack switching. Both engines use the same list, so they accept the same modules. The browser rewrite validates its input and its output. |
| Tables | At most 4 tables of at most 100 000 elements each. |
| No files, no network, no clock | The module has no imports. The host gives it nothing. WASI is not available. |
| Time | Every call gets a fuel amount (200 million work units, about one per instruction) and, on a computer, 1.5 seconds of real time. Fuel gives the same result on every computer. The time limit stops loops that do much work for little fuel. Both give a limit error. **In the browser there is fuel but no time limit** (see below). |
| Memory | Every call can use 64 MiB. A module that needs more stops with a limit error. |
| Stack | A deep or endless recursion stops with a limit error. |
| Size | The module file: 2 MiB. One function: 256 KiB. At most 5000 functions and 5000 local variables in a function (the host checks this before it compiles the module). The answer: 2 MiB, 2000 shapes, 100 000 polyline vertices. |
| Bad answers | The call fails and the drawing does not change. Examples: bad JSON, an unknown shape kind, a radius of zero or less, a number that is not finite. Numbers above 1 000 000 000 mm fail too. This holds for the shapes of `reset`, the preview and the result of a tool, and the geometry and parameters of a tool result. |
| Text | Names, authors, descriptions, labels, options, units, hints and messages lose format characters (zero-width characters and the marks that change the direction of text, such as U+202E) and control characters. A label or option has at most 100 characters, a choice at most 64 options, a record at most 32 fields. |
| Tool state | At most 64 KiB of JSON. A preview has at most 2000 shapes. A hint has at most 200 characters. |
| Same result everywhere | NaN results are made canonical, and relaxed SIMD is off. |
| Fresh instance | Every call starts in an empty instance. A call cannot see the memory of another call. |
| Trust | The user trusts the module by its SHA-256 hash, in the dialog of the app. Or the host lets agents run plug-ins. No API method gives trust. |
| Storage | A stored module is checked against its SHA-256 on load, and the sizes are capped (see [Keep modules between sessions](#keep-modules-between-sessions)). |

`plugins.info` shows the limits of a session.

The engine is `wasmtime` (Apache-2.0 with the LLVM exception), used without WASI. See [attribution.md](attribution.md).

## Threat model

What the sandbox protects against: a module that you installed and may even have trusted. It cannot reach your files, the network or the clock, it cannot use more than its fuel, memory and tables, and everything it returns is checked before it enters your drawing.

What it does not protect against, and where the boundary of trust lies:

- **Programs on your computer and scripts in the same browser origin can change the trust list and the stored modules.** The trust list (`trusted_wasm_plugins.json`) and the stored modules are ordinary files in your user data folder, and in a browser they are in the `localStorage` of the page. A program that runs as you can edit them. A script of the same origin can edit `localStorage`. Trust is therefore not a defense against such code: that code could do worse things directly. Kadakas checks the SHA-256 of every stored module against its name, so a changed module file is not run, and it checks the answer of `describe` against the stored description, but an attacker who can write the trust list can trust a module that is stored with a matching hash. Keep your user folder private (the files are created with mode 0600 on Unix).
- **The trust decision is yours, in the app.** An agent that talks to the API cannot trust or untrust a module. With the host flag `--allow-agent-scripts` an agent can run plug-ins without trust, as it can run Python. Do not start the app with this flag for agents that you do not trust.
- **The dialog shows what the module says about itself.** The name, author and description are text of the plug-in. The SHA-256 and the size are computed by Kadakas. Format characters are removed from the texts, but a module can still choose a misleading name. Compare the hash with the one that the publisher gives.
- **A trusted module can still waste time and draw wrong shapes.** In the browser there is no time limit (see above).
- **The trust list moved.** Older builds wrote it to the config folder (`trusted_wasm_plugins.json`). It is now in `plugins/` in the user data folder. There is no migration: trust that an older build wrote must be given again. This is a change in these release notes.

## API reference

All methods are in `describe` with examples. The names have `plugins` in the plural. This keeps them apart from the `plugin.*` methods of Python plug-in objects.

| Method | What it does |
|---|---|
| `plugins.info` | The engine, the limits (`time_limit_ms` is `null` in the browser), whether modules are kept (`persistent`) and the number of installed plug-ins. |
| `plugins.list` | The installed plug-ins with id, name, version, kinds, permissions, parameter schema, hash, trust and use count. |
| `plugins.install {path}` | Load a `.wasm` file. The path rules of the other file methods apply (`--allow-dir`). The extension is `.wasm`. Not in the browser build, which has no paths for agents: the user installs there. |
| `plugins.remove {plugin}` | Remove a plug-in that no object uses. |
| `plugins.place {plugin, params?, geometry, layer?, class?, name?}` | Make an object. One undo step. |
| `plugins.set_params {id, params}` | Change parameters and make the shapes again. If the module fails, the old shapes stay and the object shows the error. |
| `plugins.reset {id \| ids \| plugin}` | Make the shapes again, for example after a new build. An object that fails is reported and the others still run. |
| `plugins.object {id}` | The state of one object. |
| `plugins.preview {plugin, params?, geometry}` | Run the module and return the shapes. The drawing does not change. |
| `plugins.tool_step {plugin, event, modifiers?, state?, params?, commit?, layer?, class?, name?}` | Send one event to the tool. Returns the state, the preview, the hint and, at the end, the result. |

Error codes follow the rules of the API:

- `-32602`: bad parameters.
- `2`: a missing plug-in or object.
- `4`: a module that is not trusted, or a path that is not allowed.
- `6`: a module that went over a limit.
- `1`: a crash or a bad answer.

## Browser and server

The host is split so that the engine can change:

- `kadakas-plugin-abi` is pure Rust. It builds for `wasm32-unknown-unknown`, so it works in the browser build and in plug-ins.
- `kadakas-plugin` has two traits, `PluginRuntime` and `LoadedModule`. Everything above them (manifest checks, trust, parameter checks, shape checks, tool checks) is the same for all engines.
- Native builds (the desktop app and the server) use the `wasmtime` engine.
- The browser build uses the `WebAssembly` API of the page. The code is split in three parts: `harden.rs` rewrites a module, `metered.rs` makes the calls and finds the cause of a failure, and `web.rs` is the thin glue to `js-sys`. The first two are plain Rust. They are tested on the desktop with a test compiler on wasmtime that has no limits of its own, so only the glue (about 300 lines) has no test in this repository.

**How the browser enforces the limits.** A browser gives no fuel, no memory cap and no deadline. So the host rewrites the module before the browser compiles it:

- *Fuel.* The rewrite adds a counter (a mutable global) that starts at the fuel of the call. Each run of straight code subtracts its number of instructions at its start. At the start of every function and of every loop body the module checks the counter and executes `unreachable` when it is below zero. Endless loops, endless recursion and tail calls all pass such a point. `memory.fill`, `memory.copy`, `memory.init` and the table versions are charged by size. The host reads the counter after a trap and reports a limit error ("the module used all of its fuel").
- *Memory.* The rewrite sets the maximum of the memory to the cap (64 MiB). A module that needs more at the start does not load. `memory.grow` over the cap answers -1.
- *No imports.* A module with an import does not load, and the import object is empty.
- *Same checks as native.* Size of the module, functions, locals and the size of the answer are checked before and after, with the same code.

**What the browser does not enforce: a time limit.** Fuel counts instructions. It does not count time, and a page cannot stop a module that runs on its own thread. The browser build therefore has **no wall-clock limit**, and every call blocks the page until it ends. Only the fuel bounds the time, and the browser gives each call less fuel than a computer does:

| Call | Fuel in the browser | Fuel on a computer |
|---|---|---|
| `describe` | 5 million | 200 million |
| pointer move of a tool | 15 million | 200 million |
| `reset`, click and finish of a tool | 50 million | 200 million |

The real limits, stated plainly:

- A module that uses its whole fuel in every call is slow, but it does not hang the page for ever. How long 50 million instructions take depends on the computer: some tenths of a second on a normal one, more on a slow one.
- **A tool that is slow but succeeds blocks the page on every event.** A pointer move can cost up to 15 million instructions, and the app sends at most one move per frame, so a tool at the top of that budget makes the page jerky. The app cannot make this better without a worker.
- A tool that runs out of fuel, crashes or gives a bad answer ends its run, so it blocks the page at most once more.
- Stored modules are listed from a stored description of each module. No stored module is compiled or run when the page loads. A module is compiled and `describe` runs the first time someone needs it: when you review it, or when a trusted module is used. The answer must match the stored description.

**Follow-up (not done).** The way to a real limit is a Web Worker that runs the module, with a timeout of 1.5 seconds and `terminate()` when it is over, and `plugins.*` calls that are asynchronous for reset and tool steps. A worker can be called only asynchronously, and the `plugins.*` methods and the tool state are synchronous today, so this is a change of the API and of the app, not of the engine.

Other gaps of the browser build:

- Chrome refuses to compile a module over 4 KB with the synchronous API on the main thread. The app compiles modules with the asynchronous API before it installs them (`web::precompile`), with the same limits as the registry, and the compiled module is found by the hash of the rewritten bytes. A module that is installed without this step works only where the browser compiles synchronously.
- The `WebAssembly` features that the browser does not support make a module fail to compile ("invalid"). The allowed list above is small on purpose.
- The memory cap is reported as a limit error only when the memory is at the cap after a failure. Another failure is reported as a crash.
- The browser engine is tested in Chromium in the repository `kadakas-server` (`scripts/test-browser.sh`), with a test hook that is in the test build only. Its logic is also tested on the desktop.

Because the shapes are plain data, a drawing with plug-in objects works in the browser even without the plug-in. Only changing the parameters needs a plug-in engine.

## What is not done yet

- Permissions. The host grants none.
- A time limit in the browser (a worker setup; see above).
- An editor in the app for the options of a tool (the defaults are used).
- Curved polyline vertices in the answer.
- A plug-in folder that the app watches. Modules are installed one by one.
