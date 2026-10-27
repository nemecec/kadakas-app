# Address to model: an agent demo

You type an Estonian address. A Python script acts like an AI agent. It pulls public data and builds a georeferenced model in Kadakas. It pauses after each step, so an audience follows along.

The messages on the console are in Estonian. The run-sheet for a talk is in `AJAKAVA.md` (Estonian).

## What the script does

1. Searches the address in the Land Board address register and lets you confirm the hit.
2. Reads the building from the Building Register (EHR): footprint, height, floors, year, structure.
3. Sets the georeference to EPSG:3301 and adds the Land Board orthophoto as a background.
4. Draws the footprint and makes the building volume with one `massing.add` call (flat roof, closed solid). It adds attributes and a label.
5. Finds the plot. It uses the cadastral boundary when the optional source is present. Otherwise it offsets the footprint by 8 m and says so.
6. Optional: builds a terrain surface from the lidar grid and reports cut and fill in m3.
7. Optional: adds the neighbour buildings and trees in grey.
8. Landscape step: paving, a path, plants from a plant library and a plant schedule.
9. Building step: an extension of about 5 x 8 m on the side with the most free space, one storey lower (at least 2,7 m), on its own layer. It is one `massing.add` call, so it is one undo step.
10. Exports IFC with the georeference, a PDF and SVG sheet, a `.kad` file and PNG pictures.
11. Shows the agent activity log. In the app it highlights the extension for review.

Layers: `Hoone`, `Krunt`, `Taustakaart`, `Haljastus`, `Aruanne`, `Laiendus (agent)`, `Maapind`, `Naabrid`, `Puud`.

Every edit comes from the Python client, so Kadakas marks it with the origin `agent`. The Agent Activity panel shows the steps with Revert and Undo to here. The extension is one step on its own layer. The layer and the class are made in step 3, so Revert works on that step alone. The script prints whether the app reports the step as revertible. Reverting makes the exported files stale. Export again if you need them.

## Run it in 3 commands

You need Python 3.10 or newer and a built Kadakas (see below). The script needs no other package.

```sh
cd examples/demo-address
python3 demo_address.py "<address>" --dry                      # 1. print the plan, no network
python3 demo_address.py "<address>" --mode headless --pause 2   # 2. run without a window
python3 demo_address.py "<address>" --mode app --pause enter    # 3. run in the app, step by step
```

Replace `<address>` with a street, a number and a place, for example in the form `Street 1, Place`.

### Start the app for the live demo

```sh
cd examples/demo-address
/path/to/kadakas-app --agent-port 0 --allow-dir .
```

The `--allow-dir .` flag lets the agent write the exports into `.cache/out`. Without it the app refuses the files. With another cache folder, give that folder to `--allow-dir` and to `--cache-dir`. The default output folder is `<cache-dir>/out`, so it is inside the allowed folder. The script finds the app through its runtime file. For the headless mode, set `KADAKAS_BIN` or use `--binary /path/to/kadakas`.

Build once with `cargo build --release -p kadakas-app -p kadakas-cli`.

## Options

| Option | Meaning |
|---|---|
| `--dry` | Print what each step does. No network, no model. |
| `--cache-dir DIR` | Where every HTTP answer is stored. Default `.cache/` next to the script. |
| `--offline` | Use only the cache. A missing answer gives a clear message. |
| `--pause N` or `--pause enter` | Seconds or a key press between the steps. Default 3. |
| `--yes`, `--pick N` | Skip the question about several address hits. Take the best or hit N. |
| `--mode` | `app` uses the running app (default). `headless` starts `kadakas serve` without a window. `auto` tries the app and warns loudly when it falls back. |
| `--out-dir DIR` | Where exports and pictures go. Default `<cache-dir>/out`. |
| `--map-layer` | `of10000` (orthophoto, default) or `MA-ALUS` (base map). |
| `--no-extras` | Skip the optional sources. |
| `--extras-budget S` | Seconds that all optional sources share. Default 20. Use a large value for the first online run. |

## Pre-warm the cache for an offline demo

Run the script once with the network, for each address that you show. Use a large `--extras-budget` for that run. The cache then holds every answer, also the answers of the optional sources. Typing the address in another case or without commas finds the same entry.

```sh
python3 demo_address.py "<address>" --mode headless --pause 0 --yes --extras-budget 150
python3 demo_address.py "<address>" --offline --mode headless --pause 0 --yes   # check: no network needed
```

Keep the cache folder private. It holds data about real buildings. Use `--cache-dir` with a folder outside the repository for real addresses. The default folder is ignored by git.

## Data and assumptions

| Source | Used for | Terms |
|---|---|---|
| Land Board address search | Address, building code, reference point | Land Board open data licence |
| Building Register (EHR) open API | Footprint, height, floors, year, structure | No licence text found. See `docs/attribution.md` |
| Land Board WMS | Orthophoto or base map | Land Board open data licence |

Heights come with the rules below. The Land Board licence needs the source and the age of the data on any published result. The script writes both into the report object of the model. See `docs/attribution.md`.

The register often has no height. The script then says so aloud and uses a documented default.

- Height missing and floors known: 3,0 m per floor plus 1,5 m for the roof.
- Height and floors missing: 6,0 m.
- Floors missing: height divided by 3,0 m.
- Footprint missing: a rectangle from length and width, or 10 x 8 m, around the address point.
- Plot boundary missing: the footprint rectangle offset by 8 m.

Each assumption is printed, stored in the `Eeldused` field of the building record and listed in the report object.

## Optional sources

`sources_extra.py` gives three more functions: `plot_boundary`, `terrain_grid` and `nearby_features`. The script imports them softly. Without the file, the steps print a short note and the demo goes on. Cache and offline settings are passed on to them.

## Tests

```sh
cd tests/behavior
KADAKAS_BIN=/path/to/kadakas KADAKAS_SKIP_BUILD=1 python3 -m pytest test_demo_address.py
```

The tests use only invented data from `examples/demo-address/fixtures`. They start a headless session and run the whole script.

## Limits

- The model is a massing, not a design.
- Underground utilities are not public data, so the model has none.
- Heights and some other fields are often missing in the register.
- Each call of the script is one step in the agent log. A run makes about 50 steps. Only steps that make one object and nothing else are revertible. The extension is such a step. The building has attributes, so use Undo to here for it.
- The Kadakas plant library has no built-in species. The script creates six plant styles.

## Troubleshooting

| Problem | What to do |
|---|---|
| `Kadakase rakendus ei vasta` | Start the app with `--agent-port 0`. Or use `--mode headless`. |
| Export refused: outside the folders | Start the app with `--allow-dir` for the output folder. |
| `Võrguta režiim: ... pole vahemälus` | Run once without `--offline` to fill the cache. |
| No answer from a service | Run again. Or switch to the cache with `--offline`. |
| The document is not empty | Choose File > New in the app. Run the script again. |
| Wrong address hit | Run with `--pick N` or answer the question. |
