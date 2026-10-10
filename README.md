# Kadakas

Kadakas is a 2D CAD program for landscape design, written in Rust. Every change to a drawing is a
serializable, undoable command. The desktop app, scripts and AI agents all send the same commands.

- `crates/kadakas-core`: document model, commands with undo, SVG render, JSON-RPC API. No UI code.
- `crates/kadakas-io`: file formats. Imports DXF, Shapefile and GeoJSON, exports DXF and GeoJSON. Reads and writes DWG through a converter that you install (`docs/dwg.md`). The core never parses files.
- `crates/kadakas-ifc`: IFC4 export and IFC2x3, IFC4 and IFC4x3 import (a streaming STEP reader, geometry, mapping).
- `crates/kadakas-script`: scripting. It finds Python, runs scripts with limits, and holds the plug-in object reset protocol.
- `python/kadakas`: the Python package for scripts and automation (`import kadakas`).
- `crates/kadakas-cli`: the `kadakas` command for scripts and agents (JSON-RPC over stdio, MCP server, SVG render).
- `crates/kadakas-app`: the desktop app (egui). It can also serve the API over TCP to agents.

Documents are JSON files with the extension `.cdk`. Older files named `*.cdk.json` still open, and File > Save keeps their name (File > Save As offers `.cdk`). Units are millimeters, the Y axis points up, angles are in radians.
See `docs/feature-spec.md` for the plan.

## Build

You need a recent stable Rust toolchain. On Linux you also need `libxkbcommon-dev` and `libgtk-3-dev`.

```sh
cargo build --release
cargo test --workspace
```

## Install

Installers for macOS (`.dmg`), Windows (`.exe`, `.msi`) and Linux (`.AppImage`, `.deb`) come with each release.
They are not signed yet, so your system may warn you at the first start. The installer also puts the `kadakas`
command line tool next to the app. `docs/release.md` has the paths and the build steps.

## Autosave and recovery

The app saves a recovery copy every 5 minutes (when the drawing changed) or after 50 operations, and keeps the last 5
versions. They go to a folder of your user account, never over your file (File > Preferences... changes all of this, and has
an opt-in to save over your own file too). After a crash, the app offers the last copy at the next start: open it, discard it
or show its folder. A normal exit removes the copies. The folder is `~/Library/Application Support/kadakas/recovery` on macOS,
`%LOCALAPPDATA%\kadakas\recovery` on Windows and `~/.local/state/kadakas/recovery` on Linux (`KADAKAS_RECOVERY_DIR` changes it).

## Run

```sh
# Desktop app
cargo run --release -p kadakas-app

# Headless: one JSON-RPC 2.0 request per line on stdin, one response per line on stdout
cargo run --release -p kadakas-cli -- serve [file.cdk.json]

# Render a document to SVG
cargo run --release -p kadakas-cli -- render drawing.cdk.json -o drawing.svg
```

## Web

The app also runs in a browser (WebGPU, or WebGL2 as the fallback) with fewer features. Build it with
`scripts/build-web.sh`. `docs/web.md` lists what works and what does not.

Import and export (File > Import... and File > Export... in the app, `import.file` and `export.file` in the API):

```sh
kadakas import site.dxf -o drawing.cdk.json                                # DXF layers become classes
kadakas import trees.geojson -o drawing.cdk.json --into drawing.cdk.json  # GIS data sets the georeference
kadakas import site.dxf -o d.cdk.json --options '{"layer_mode": "layers", "unit": "m"}'
kadakas export drawing.cdk.json -o out.dxf --options '{"unit": "m"}'
kadakas export drawing.cdk.json -o site.ifc                                 # IFC4; the first export stores a project GUID in the file
kadakas import model.ifc -o drawing.cdk.json                                # IFC2x3, IFC4 or IFC4x3
kadakas import survey.dwg -o drawing.cdk.json                               # needs a converter, see docs/dwg.md
```

An import is one undo step and returns a report of what it skipped and why. GIS files are converted to the
document CRS with `proj4rs`. The document keeps a georeference (CRS and origin offset), so geometry stays near (0, 0).
A drawing whose center is more than 1 km from (0, 0) is centered, also when the file has no CRS (DXF in meters, a
shapefile without `.prj`). The shift is saved as the georeference origin, with an empty CRS. Use the option
`"origin": "file"` to keep the numbers of the file. Holes of polygons stay as closed polylines that point to their outer
ring (`_hole_of`), and the GeoJSON export writes them back as interior rings. DXF text keeps its rotation in the object
data (`rotation_deg`). Old files are decoded with their code page (`$DWGCODEPAGE`, shapefile `.cpg`).

The DXF export writes a release 2004 file with all tables, handles and owner handles, and snaps line weights to the values
that DXF allows. To check it with ezdxf: `uv run --with pytest --with ezdxf pytest tests/behavior -q`.

A server for agents (`kadakas serve`, `kadakas mcp`, the app with `--agent-port`) only reads and writes in the folder of
the open document and in the folders given with `--allow-dir DIR` (repeat the flag; default: the folder where it started).
This covers `import.file`, `export.file`, `file.open` and `file.save`. Paths are resolved first, so `..` and links do not
leave those folders. The extension must match the format, and `export.file` needs `"overwrite": true` to replace a file
(the old file stays as `<name>.bak`). The File > Import and Export dialogs of the app are not limited.

Agents cannot use the network unless the owner allows it: the map services (`gis.wms.*`, `gis.wmts.*`, `gis.wfs.*`) fetch
only addresses given with `--allow-url HOST_OR_PREFIX` (repeat the flag; `maps.example.org`, `*.example.org` or
`https://maps.example.org/wms/`; default: none). The Map Sources dialog of the app is the user's own and is not limited.

Saves are atomic: the app and the CLI write a temporary file and rename it. The previous file stays as `<name>.bak`.

## Agents

Send the `describe` method first. It lists all methods and example commands.

### Headless

`kadakas serve` and `kadakas mcp` work on their own document. Use `file.open` and `file.save` (in MCP, methods of the `document` tool) to load and store it.

### The live app

Start the app with an agent port. Port 0 picks a free port.

```sh
cargo run --release -p kadakas-app -- --agent-port 0     # or set KADAKAS_AGENT_PORT=0
```

The app listens on 127.0.0.1 only. At start it creates a random 256-bit token and writes
`{"port": N, "token": "..."}` to a file that only you can read. The app deletes the file when it exits.

| System | Runtime file |
|---|---|
| Linux | `$XDG_RUNTIME_DIR/kadakas/agent.json`, else `~/.local/state/kadakas/agent.json` |
| macOS | `$XDG_RUNTIME_DIR/kadakas/agent.json`, else `~/Library/Application Support/kadakas/agent.json` |
| Windows | `%LOCALAPPDATA%\kadakas\agent.json` |

The first line on every connection must be an `auth` request. Anything else closes the connection:

```json
{"jsonrpc":"2.0","id":0,"method":"auth","params":{"token":"<token>"}}
```

The first byte must be `{`. Lines are at most 16 MiB, and at most 4 clients can be connected at once.

The CLI does this for you:

```sh
kadakas serve --connect                          # find the app through the runtime file
kadakas serve --connect 127.0.0.1:7878 --token T # explicit address; the token may also come from the file
```

### Claude Code (MCP)

```sh
# Live app (start the app with --agent-port first)
claude mcp add kadakas -- /path/to/kadakas mcp --connect

# Headless, with its own document
claude mcp add kadakas -- /path/to/kadakas mcp
```

The server lists about 20 tools, one per domain: `describe`, `document`, `objects`, `selection_undo`, `reports`,
`render_view`, `materials_styles`, `plants`, `site`, `grading`, `hardscape_landscape`, `irrigation`, `annotation`,
`sheets_print`, `data_worksheets`, `resources_symbols`, `io`, `gis` and `scripting`. Every tool takes
`{method, params}`: `method` is a method of the API (for example `objects.list`) and `params` are its parameters.
The description of each tool lists its methods. Call `describe` first. For example:

```json
{"name": "objects", "arguments": {"method": "objects.list", "params": {"kind": "plant"}}}
{"name": "document", "arguments": {"method": "execute", "params": {"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 500}}}}}
```

The `document` tool also has the file methods `file.open` and `file.save`. Layers and classes have no methods of
their own: change them with commands of `execute`. Many methods change the drawing, not only `execute` (for example
`plants.place`, `hardscape.add` and `import.file`). The tools carry MCP annotations: `describe`, `objects` and
`reports` are read-only. The others may change or delete data (`destructiveHint`), and `gis` may use the network.

Some clients work better with one tool per method. Start the server with `--mcp-flat` to get the older listing of
about 280 tools. The tool names are the API methods with `.` replaced by `_` (for example `objects_list`), plus
`file_open` and `file_save`, and the arguments are the parameters of the method.

```sh
claude mcp add kadakas -- /path/to/kadakas mcp --mcp-flat
```

## Scripting and plug-in objects

Control Kadakas from Python, run `.py` files from **File > Scripts...** (with a console), and make parametric
plug-in objects whose shapes a short Python script draws again when a parameter changes. Python is not embedded:
Kadakas starts `python3` (or `uv run`). Scripts run only when you start them; agents may run scripts only with
`--allow-agent-scripts`.

```sh
uv pip install -e python
```

```python
import kadakas

with kadakas.open() as doc:                 # or kadakas.connect() for the running app
    doc.plugins.install_example("Bench")
    bench = doc.plugins.place("Bench", at=[2000, 1000], params={"length": 2400})
    with doc.undo_group():                   # one undo step
        doc.add.circle([0, 0], 300)
    doc.render_svg("plan.svg")
```

See `docs/scripting.md`.

## Keyboard shortcuts

The default keys follow Vectorworks 2026: `X` Selection, `1` Text, `2` Line, `4` Rectangle, `5` Polyline, `6` Circle,
`Cmd/Ctrl+Z` undo, `Cmd/Ctrl+Y` redo, `Cmd/Ctrl+6` zoom to fit, arrows pan, Space plus drag pans, and the back quote key
suspends snapping. While you draw a line, rectangle, circle or polyline, `Tab` moves into the Data bar fields
(length and angle, width and height, radius, or X and Y). `Tab` and `Shift+Tab` change field, `Enter` creates the object,
`Esc` returns to the canvas. Single keys never fire while you type in a text field.

Put a `keymap.json` in your config folder to change keys (on macOS `~/Library/Application Support/kadakas/keymap.json`).
The full table, with the status of each key, is in `docs/keyboard-shortcuts.md`.

## Tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p kadakas-cli && uv run --with pytest pytest tests/behavior -q
KADAKAS_BIN=target/debug/kadakas uv run --no-project --with pytest pytest python/tests   # the Python package
cargo test -p kadakas-core --release --test perf -- --ignored --nocapture   # speed on 50,000 objects
cargo llvm-cov -p kadakas-core --summary-only
cargo llvm-cov -p kadakas-io --summary-only
```

## License

Licensed under either of Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)) or MIT license
([LICENSE-MIT](LICENSE-MIT)) at your option.

Third-party licenses are in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt). [docs/attribution.md](docs/attribution.md) lists the libraries, icons, fonts, data and sources that we use.

Vectorworks is a trademark of Vectorworks, Inc. Kadakas is an independent project and is not affiliated with, endorsed by, or sponsored by Vectorworks, Inc. AutoCAD, DWG and DXF are trademarks of Autodesk, Inc. We name these products only to describe the file formats that Kadakas reads or writes, or to compare workflows. Kadakas is not affiliated with their owners.

## IFC import

`import.file` with a `.ifc` file (or format `ifc`) reads IFC2x3, IFC4 and IFC4x3 in one undo step. The reader indexes the file
once, in blocks, and keeps 24 bytes per entity in memory (the file stays on disk). It parses an entity when it needs it. So large files work (caps: 1 GiB, 20 million entities). Elements become objects:

- Slabs, coverings, walls and every other element with a body become a generic 3D mesh (`mesh3d`). The IFC class is in
  `data.ifc_class`, and the GlobalId, name and type are in `data.ifc`. Nothing becomes a hardscape.
- Terrain becomes a site model (its vertices are the points, option `terrain: "mesh"` keeps the triangles). Vegetation becomes a
  landscape area (thin flat extrusion, arcs stay exact) or a plant (size from the geometry), else a mesh.
- A mapped item that is used twice or more and has only upright extrusions becomes a symbol with instances.
- Storeys become design layers with their elevation. Presentation layers become classes. Property sets and quantity sets become
  records (option `property_sets`: `records`, `data` or `none`). Colors, units (SI prefixes, feet) and placements are read.
- `IfcMapConversion` sets the georeference of an empty document, or moves the file into the georeference of the document when the CRS is the same.
- Geometry that is not supported is listed in the report. A bounding box stands in for it when the file has one.
  Openings are not cut. `describe` lists the options (`io.ifc.import`). Test files are in `tests/fixtures/ifc`.

## DWG

`import.file` and `export.file` accept `.dwg` files when a converter is installed (ODA File Converter, or the LibreDWG tools
`dwg2dxf` and `dxf2dwg`). Kadakas runs it in a temporary folder with a time limit and uses the DXF import and export.
See [docs/dwg.md](docs/dwg.md) for install hints and licenses.

## IFC4 export

`export.file` with format `ifc` (or a `.ifc` file name) writes IFC4 ADD2 TC1 as a STEP file, one entity at a time.
The file has an `IfcProject` (mm, m2, m3), an `IfcSite` with `IfcMapConversion`, `IfcProjectedCRS` and
latitude and longitude from the document georeference, and no `IfcBuilding` (with the option `layers_as_storeys`, a default
building holds one storey per design layer). Site models become `IfcGeographicElement` TERRAIN (triangulated face set),
hardscapes `IfcSlab` BASESLAB like the Vectorworks default (extruded solid, or a mesh when sloped or draped; style
components are an `IfcMaterialLayerSet`), landscape areas and plants `IfcGeographicElement` with ObjectType VEGETATION
(IFC4 has no VEGETATION type). 2D objects are skipped and listed in the report (option `include_2d` writes `IfcAnnotation`).
Classes become presentation layers, fill colors become surface styles. Property sets come from the Data Manager map, record
formats (`ePset_<record>`), `Pset_SlabCommon` and `Qto_SlabBaseQuantities`. GlobalIds come from the project GUID and the
object IDs and stay the same in every export. Every new document has a project GUID. A file from an older version gets one at load, from its content, in memory only. An export never changes the document or the input file. `project.new_guid` gives a copy of a template its own GUID. True north in the context includes the meridian convergence of the CRS at the origin. Check a file with IfcOpenShell:
`uv run --with pytest --with ezdxf pytest tests/behavior/test_ifc_export.py -q` (it runs `ifcopenshell.validate`).
