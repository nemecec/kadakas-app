# Cadektor in the browser

The web build runs the app in a browser tab. It is a small version of the desktop app.
It is for a quick look, a demo and light edits. The desktop app stays the main product.

## Build and run

```sh
scripts/build-web.sh           # release build into dist-web/
scripts/build-web.sh --serve   # dev build, local server on http://127.0.0.1:8080
```

The script installs `trunk` once. Trunk uses `Trunk.toml` and `web/index.html`. You need the
`wasm32-unknown-unknown` Rust target (the script adds it). CI checks the code with
`cargo check -p cadektor-app --target wasm32-unknown-unknown` and builds with `trunk build`.

Host the `dist-web` folder on any static web server. No server code is needed.

## What works

- Drawing, editing, undo and redo, snaps, layers, classes, plants, hardscapes, grading, irrigation, sheets on screen, and the 3D view.
- Graphics: WebGPU if the browser has it, WebGL2 if not. If neither exists, the page shows an error message.
- File > Open picks a `.cdk.json` file from your computer. File > Save and Save As download the file. File > Export SVG downloads an SVG.
- Autosave keeps one recovery copy in the local storage of the browser. After a crash or a closed tab, the app offers it at the next start: open it or discard it. The copy is removed when you use File > Save.

## What does not work (and why)

- No agent server, no MCP, no `--agent-port`. A browser page cannot open a network port.
- No Python scripts and no Scripts palette. A browser cannot start programs.
- No map services (WMS, WMTS, WFS) and no coordinate readout or CRS change. These use native network and projection code. A later version may use the `fetch` call of the browser.
- No import or export of DXF, Shapefile, GeoJSON or IFC from the menu. The file format code is native only for now.
- No PDF export and no print of sheets. Sheets show on screen only.
- Files have no path. The browser cannot write to a file you opened. You save by download, and you get a new file each time.
- Autosave is limited by the browser: about 5 MB per site. A large drawing may not fit. The Preferences dialog shows the last error. There is no clean-exit detection in a tab, so the copy stays until you save, open or discard it.
- Keyboard keys that the browser keeps for itself (for example `Ctrl+W`, `Ctrl+T`, `Ctrl+N`) never reach the app.
- Speed is lower than the desktop app. Big 3D scenes can be slow.
- The page was only compiled in our checks. Run it in your own browsers before you rely on it.
