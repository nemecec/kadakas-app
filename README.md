# Cadektor

Cadektor is a 2D CAD program for landscape design, written in Rust. Every change to a drawing is a
serializable, undoable command. The desktop app, scripts and AI agents all send the same commands.

- `crates/cadektor-core`: document model, commands with undo, SVG render, JSON-RPC API. No UI code.
- `crates/cadektor-io`: file formats. Imports DXF, Shapefile and GeoJSON, exports DXF and GeoJSON. The core never parses files.
- `crates/cadektor-cli`: the `cadektor` command for scripts and agents (JSON-RPC over stdio, MCP server, SVG render).
- `crates/cadektor-app`: the desktop app (egui). It can also serve the API over TCP to agents.

Documents are JSON files (`*.cdk.json`). Units are millimeters, the Y axis points up, angles are in radians.
See `docs/feature-spec.md` for the plan.

## Build

You need a recent stable Rust toolchain. On Linux you also need `libxkbcommon-dev` and `libgtk-3-dev`.

```sh
cargo build --release
cargo test --workspace
```

## Run

```sh
# Desktop app
cargo run --release -p cadektor-app

# Headless: one JSON-RPC 2.0 request per line on stdin, one response per line on stdout
cargo run --release -p cadektor-cli -- serve [file.cdk.json]

# Render a document to SVG
cargo run --release -p cadektor-cli -- render drawing.cdk.json -o drawing.svg
```

Import and export (File > Import... and File > Export... in the app, `import.file` and `export.file` in the API):

```sh
cadektor import site.dxf -o drawing.cdk.json                                # DXF layers become classes
cadektor import trees.geojson -o drawing.cdk.json --into drawing.cdk.json  # GIS data sets the georeference
cadektor import site.dxf -o d.cdk.json --options '{"layer_mode": "layers", "unit": "m"}'
cadektor export drawing.cdk.json -o out.dxf --options '{"unit": "m"}'
```

An import is one undo step and returns a report of what it skipped and why. GIS files are converted to the
document CRS with `proj4rs`. The document keeps a georeference (CRS and origin offset), so geometry stays near (0, 0).

Saves are atomic: the app and the CLI write a temporary file and rename it. The previous file stays as `<name>.bak`.

## Agents

Send the `describe` method first. It lists all methods and example commands.

### Headless

`cadektor serve` and `cadektor mcp` work on their own document. Use `file.open` and `file.save` (MCP tools
`file_open` and `file_save`) to load and store it.

### The live app

Start the app with an agent port. Port 0 picks a free port.

```sh
cargo run --release -p cadektor-app -- --agent-port 0     # or set CADEKTOR_AGENT_PORT=0
```

The app listens on 127.0.0.1 only. At start it creates a random 256-bit token and writes
`{"port": N, "token": "..."}` to a file that only you can read. The app deletes the file when it exits.

| System | Runtime file |
|---|---|
| Linux | `$XDG_RUNTIME_DIR/cadektor/agent.json`, else `~/.local/state/cadektor/agent.json` |
| macOS | `$XDG_RUNTIME_DIR/cadektor/agent.json`, else `~/Library/Application Support/cadektor/agent.json` |
| Windows | `%LOCALAPPDATA%\cadektor\agent.json` |

The first line on every connection must be an `auth` request. Anything else closes the connection:

```json
{"jsonrpc":"2.0","id":0,"method":"auth","params":{"token":"<token>"}}
```

The first byte must be `{`. Lines are at most 16 MiB, and at most 4 clients can be connected at once.

The CLI does this for you:

```sh
cadektor serve --connect                          # find the app through the runtime file
cadektor serve --connect 127.0.0.1:7878 --token T # explicit address; the token may also come from the file
```

### Claude Code (MCP)

```sh
# Live app (start the app with --agent-port first)
claude mcp add cadektor -- /path/to/cadektor mcp --connect

# Headless, with its own document
claude mcp add cadektor -- /path/to/cadektor mcp
```

The MCP tools are the API methods with `.` replaced by `_` (for example `objects_list`), plus `file_open`
and `file_save`. `execute` is the only tool that changes the drawing.

## Keyboard shortcuts

The default keys follow Vectorworks 2026: `X` Selection, `1` Text, `2` Line, `4` Rectangle, `5` Polyline, `6` Circle,
`Cmd/Ctrl+Z` undo, `Cmd/Ctrl+Y` redo, `Cmd/Ctrl+6` zoom to fit, arrows pan, Space plus drag pans, and the back quote key
suspends snapping. While you draw a line, rectangle, circle or polyline, `Tab` moves into the Data bar fields
(length and angle, width and height, radius, or X and Y). `Tab` and `Shift+Tab` change field, `Enter` creates the object,
`Esc` returns to the canvas. Single keys never fire while you type in a text field.

Put a `keymap.json` in your config folder to change keys (on macOS `~/Library/Application Support/cadektor/keymap.json`).
The full table, with the status of each key, is in `docs/keyboard-shortcuts.md`.

## Tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p cadektor-cli && uv run --with pytest pytest tests/behavior -q
cargo test -p cadektor-core --release --test perf -- --ignored --nocapture   # speed on 50,000 objects
cargo llvm-cov -p cadektor-core --summary-only
cargo llvm-cov -p cadektor-io --summary-only
```

## License

Licensed under either of Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)) or MIT license
([LICENSE-MIT](LICENSE-MIT)) at your option.
