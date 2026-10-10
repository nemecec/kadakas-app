# Plug-ins

This guide shows how to write a plug-in for Kadakas and how to use one.

A plug-in is a small WebAssembly program (a `.wasm` file). It makes a parametric object, such as a raised planter. You give it parameters and a place. It gives back shapes.

Plug-ins run in a sandbox. They cannot read files, use the network or read the clock. They run the same way on the desktop, on a server and (later) in the browser.

Python stays the tool for automation scripts. See [scripting.md](scripting.md). Python plug-in objects still work. If the object must run the same everywhere, use a WebAssembly plug-in. It is also the better choice for objects that you share with other people. They need not trust a Python script.

## Contents

- [How it works](#how-it-works)
- [Use a plug-in](#use-a-plug-in)
- [Write a plug-in](#write-a-plug-in)
- [The ABI](#the-abi)
- [Safety and limits](#safety-and-limits)
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
- The user trusts a module in the app. The trust file is `trusted_wasm_plugins.json` in the config folder. (The dialog is not in the app yet. See [What is not done yet](#what-is-not-done-yet).)
- No API method adds trust. Agents can run plug-ins only when the host allows it: start `kadakas serve`, `kadakas mcp` or the app with `--allow-agent-scripts`. This is the same rule as for Python scripts. `plugins.list` shows `trusted` (a user trusted this exact module) and `runnable` (trusted, or the flag is set).
- A sandboxed module is safer than a Python script, but it can still draw wrong shapes or waste time. That is why the user still approves each module.

## Upgrade a plug-in

Install the new build with `plugins.install`. It replaces the old build with the same id. The objects keep their shapes. `plugins.object` shows `stale: true` for them, and `plugins.install` says how many there are. The new build must be trusted again (it has a new hash). Then call `plugins.reset {plugin}`. This regenerates all objects of the plug-in in one undo step.

When a reset reads the old parameters of an object, it drops the ones that the new schema does not know, and it uses the default for the ones that no longer fit (for example, a value outside the new range). `plugins.set_params` with `null` for a name puts the default back.

A reset replaces only the shapes that the plug-in made. Shapes that you added to the group stay. The class of the shapes stays too. A move of the group is followed, as long as you move the shapes of the plug-in.

Installed modules are not kept between sessions yet. When you start a new session, install the module again. The shapes of existing objects stay in the drawing. If the module is not installed, the objects show their shapes and `plugins.object` says `installed: false`.

## Write a plug-in

You can use any language that makes WebAssembly. The examples use Rust, with the crate `kadakas-plugin-abi`. It has the manifest and message types, so you do not write JSON by hand.

The example `plugins/raised-planter` is a good start. It has about 120 lines.

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

Parameter types are `number`, `length` (mm), `text`, `bool`, `choice` (with `options`), `color` (`#rrggbb`) and `record` (a group of fields). Kadakas checks every value before it calls your module. Your module always gets all parameters, with defaults filled in.

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

Rules for the answer:

- Return at least one shape.
- Units are millimeters. The Y axis points up. Angles are radians.
- Shape kinds: `line`, `rect`, `circle`, `arc`, `polyline` and `text`. A polyline has plain corner points (no curves yet).
- Colors are `[r, g, b, a]`. Leave them out to use the class colors. `color_rgba("#6b4f3a")` turns a color parameter into this form.
- If the module cannot make the object (for example, it is too small), return `Err(message)`. The user sees your message. The old shapes stay.
- Do not use `panic!`. A panic ends the call with a crash message and no detail.
- The exports of the module are called `alloc`, `dealloc`, `describe` and `reset`. Do not use these names for your own functions. That is why the example calls its function `make_shapes`.

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

The ABI (application binary interface) is the contract between the host and the module. This is version `1`. The host refuses a module with another version.

The module has these exports and no imports:

| Export | Meaning |
|---|---|
| `memory` | The linear memory of the module. |
| `alloc(len: u32) -> u32` | Reserve `len` bytes. Return the address. |
| `dealloc(ptr: u32, len: u32)` | Free a block. The Kadakas host does not call it (see below), but the export must exist. |
| `describe() -> u64` | Return the manifest as JSON. |
| `reset(ptr: u32, len: u32) -> u64` | Read the request at `ptr`. Return the answer as JSON. |

A `u64` result holds an address in the high 32 bits and a length in the low 32 bits. A result of `0` means "no answer", which is an error.

The host calls `reset` in four steps. It calls `alloc`. It writes the request into the memory. It calls `reset`. Then it reads the answer from the memory. Every call runs in a new instance, so nothing stays in memory between calls. The host drops the instance after the call, so it does not call `dealloc`. Your module needs no global state.

The manifest:

```json
{
  "id": "org.kadakas.raised-planter",
  "name": "Raised planter",
  "version": "1.0.0",
  "abi_version": 1,
  "description": "A raised planter.",
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

`kinds` can be `object` or `tool`. The kind `tool` is reserved. ABI version 1 has no tool messages yet. The host lists such a plug-in but does not run it.

`permissions` can name `read_document`, `filesystem`, `network` or `clock`. ABI version 1 has no calls from the module to the host. The host grants none. It refuses a module that asks.

Version rules: a change that breaks old modules gets a new ABI version. New optional fields in the JSON do not. The host ignores nothing silently: an unknown shape kind or field in the answer is an error.

## Safety and limits

| Protection | How |
|---|---|
| No files, no network, no clock | The module has no imports. The host gives it nothing. WASI is not available. |
| Time | Every call gets a fuel amount (200 million work units, about one per instruction) and 1.5 seconds of real time. Fuel gives the same result on every computer. The time limit stops loops that do much work for little fuel, such as `memory.fill`. Both give a limit error. |
| Memory | Every call can use 64 MiB. A module that needs more stops with a limit error. |
| Stack | A deep or endless recursion stops with a limit error. |
| Size | The module file: 2 MiB. One function: 256 KiB. At most 5000 functions and 5000 local variables in a function (the host checks this before it compiles the module). The answer: 2 MiB, 2000 shapes, 100 000 polyline vertices. |
| Bad answers | The call fails and the drawing does not change. Examples: bad JSON, an unknown shape kind, a radius of zero or less, a number that is not finite. Numbers above 1 000 000 000 mm fail too. |
| Same result everywhere | NaN results are made canonical, and relaxed SIMD is off. |
| Fresh instance | Every call starts in an empty instance. A call cannot see the memory of another call. |
| Trust | The user trusts the module by its SHA-256 hash. Or the host lets agents run plug-ins. |

`plugins.info` shows the limits of a session.

The engine is `wasmtime` (Apache-2.0 with the LLVM exception), used without WASI. See [attribution.md](attribution.md).

## API reference

All methods are in `describe` with examples. The names have `plugins` in the plural. This keeps them apart from the `plugin.*` methods of Python plug-in objects.

| Method | What it does |
|---|---|
| `plugins.info` | The engine, the limits and the number of installed plug-ins. |
| `plugins.list` | The installed plug-ins with id, name, version, kinds, permissions, parameter schema, hash, trust and use count. |
| `plugins.install {path}` | Load a `.wasm` file. The path rules of the other file methods apply (`--allow-dir`). The extension is `.wasm`. |
| `plugins.remove {plugin}` | Remove a plug-in that no object uses. |
| `plugins.place {plugin, params?, geometry, layer?, class?, name?}` | Make an object. One undo step. |
| `plugins.set_params {id, params}` | Change parameters and make the shapes again. If the module fails, the old shapes stay and the object shows the error. |
| `plugins.reset {id \| ids \| plugin}` | Make the shapes again, for example after a new build. An object that fails is reported and the others still run. |
| `plugins.object {id}` | The state of one object. |
| `plugins.preview {plugin, params?, geometry}` | Run the module and return the shapes. The drawing does not change. |

Error codes follow the rules of the API:

- `-32602`: bad parameters.
- `2`: a missing plug-in or object.
- `4`: a module that is not trusted, or a path that is not allowed.
- `6`: a module that went over a limit.
- `1`: a crash or a bad answer.

## Browser and server

The host is split so that the engine can change:

- `kadakas-plugin-abi` is pure Rust. It builds for `wasm32-unknown-unknown`, so it works in the browser build and in plug-ins.
- `kadakas-plugin` has two traits, `PluginRuntime` and `LoadedModule`. Everything above them (manifest checks, trust, parameter checks, shape checks) is the same for all engines.
- Native builds (the desktop app and the server) use the `wasmtime` engine.
- The browser build has a stub engine. It says "plug-ins do not run in the browser build yet". A later engine can call the `WebAssembly` API of the browser behind the same two traits. Every call runs in a new instance, and the interface is bytes in and bytes out. So such an engine needs about 100 lines.
- Because the shapes are plain data, a drawing with plug-in objects works in the browser today. Only changing the parameters needs a plug-in engine.

## What is not done yet

- The kind `tool`. The manifest can say it, but the ABI has no messages for pointer events.
- The dialog in the app to trust a module. The function is ready (`kadakas_api::trust_wasm_plugin`). The window is not.
- A parameter editor in Object Info for WebAssembly plug-in objects.
- A browser engine.
- Keeping installed modules between sessions, and a plug-in folder.
- Permissions. The host grants none.
- Curved polyline vertices in the answer.
