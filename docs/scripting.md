# Scripting and plug-in objects

This guide shows how to control Kadakas with Python, and how to make your own parametric objects.

You can do three things:

1. Write a Python program that draws, queries and exports. It can run on its own or inside the app.
2. Run `.py` files from the **Scripts** palette in the app. They work on the open drawing, live.
3. Make a **plug-in object**: an object with parameters. A short Python script draws its shapes again each time you change a parameter.

Python is not part of Kadakas. Kadakas starts the Python that is on your computer.

## Contents

- [Getting started](#getting-started)
- [The Python package](#the-python-package)
- [Scripts palette](#scripts-palette)
- [Plug-in objects](#plug-in-objects)
- [Security](#security)
- [API reference](#api-reference)
- [Examples](#examples)
- [Keep the package in step with the API](#keep-the-package-in-step-with-the-api)

## Getting started

You need Python 3.10 or newer and the `kadakas` program (build it with `cargo build -p kadakas-cli`).

Install the package. It has no other dependencies.

```sh
uv pip install -e python        # or: pip install -e python
```

Write your first program:

```python
import kadakas

with kadakas.open() as doc:                 # starts "kadakas serve" for you
    doc.add.line([0, 0], [5000, 0])
    doc.add.circle([2500, 0], 300)
    print(doc.summary()["objects_by_kind"])  # {'circle': 1, 'line': 1}
    doc.render_svg("plan.svg")
```

Units are millimeters. The Y axis points up. Angles are in radians.

If the `kadakas` program is not on PATH, set `KADAKAS_BIN` or pass `binary="..."` to `kadakas.open()`.

### Two ways to connect

| Call | What it does |
|---|---|
| `kadakas.open(path=None)` | Starts its own `kadakas serve` with its own document. The process ends when you close the document. |
| `kadakas.connect()` | Joins the running app. It uses a TCP connection and a token. |

`connect()` finds the app in this order:

1. The arguments: `kadakas.connect("127.0.0.1:7878", token)`.
2. The environment variables `KADAKAS_ADDR` and `KADAKAS_TOKEN`. The app sets them for every script it starts.
3. The runtime file of the app (see the README, section "The live app"). Start the app with `--agent-port 0` to have one.

You see the live drawing. When you change something in the app, your script sees it on the next call.

## The python package

The package is `python/kadakas/`. It is pure Python and has type hints.

```python
import kadakas

doc = kadakas.connect()

# Layers and classes
planting = doc.add_layer("Planting", elevation=0)
trees = doc.add_class("Trees", pen=[0, 120, 0, 255])
planting.activate()
trees.activate()

# Objects
circle = doc.add.circle([1000, 1000], 400)
circle.modify(radius=500)
circle.move(200, 0)
print(circle.kind, circle.radius)

# Queries
big = doc.objects(kind="circle", where=lambda o: o.radius > 300)
on_layer = doc.objects(layer="Planting")

# Selection (the app shows it)
doc.selection.set(big)

# Plants
oak = doc.plants.create_style("Oak", spread=4000, height=8000)["id"]
doc.plants.place("grid", style=oak, min=[0, 0], max=[20000, 12000], spacing=5000)
print(doc.report.plant_schedule()["total_count"])

# Worksheets
sheet = doc.worksheets.create("Costs", cells={"A1": 12, "A2": 30, "A3": "=A1*A2"})
print(doc.worksheets.evaluate(sheet)["cells"]["A3"])      # 360

# Files and pictures
doc.import_file("survey.geojson")
doc.export_file("plan.dxf")
doc.render_png("view.png", view="front", width=1200, height=800)
doc.save("garden.json")
```

### Undo groups

Each change is one undo step. Use `undo_group` to make several changes one step:

```python
with doc.undo_group():
    for i in range(10):
        doc.add.circle([i * 1000, 0], 300)
doc.undo()          # all ten circles go away
```

Inside the block, drawing calls wait. When the block ends, they go out together as one `batch` command. If an error happens in the block, nothing is sent.

New objects inside the block are `Pending`. Their `.id` is known only after the block. You cannot use a `Pending` object in a later call inside the same block. Calls that are not commands, for example `plants.place`, cannot be part of a group.

### IDs

Every object, layer, class and resource has an ID. An ID is a UUID text such as `0192f3c4-7b1a-7c2e-9d41-5e6f7a8b9c0d`. IDs are unique in the world, so a client that works offline never makes an ID that another client made. A file from an older format version, with number IDs, does not open: the loader says "unsupported old format".

In Python, an ID is a `str` (`kadakas.Id`). Use `kadakas.is_id(text)` to test one. Wherever a layer, class or resource is expected, you can give its ID or its name.

For tests that need IDs that you can predict, start the session with the flag `--sequential-ids`. New IDs then count up from `00000000-0000-0000-0000-000000000002`. Do not use this flag for documents that you merge with others.

### Change events

`doc.events` tells you what changed, so you need not read the document again.

```python
doc.events.subscribe(kinds=["objects"])      # kinds is optional
doc.add.circle([0, 0], 300)
for message in doc.events.take():
    change = message["params"]
    print(change["op"], change["origin"], change["added"]["objects"])
```

A change set has these fields: `revision`, `op` (the command name, or `undo`, `redo` or `rollback`), `origin` (`user`: the person at the app; `agent`: any program that calls the API, your Python script too; `script`: a plug-in object that its script generated again), `added`, `changed` and `removed` (each lists the IDs of `objects`, `layers`, `classes` and `resources`), and `document` (flags: `active_layer`, `active_class`, `georef`, `drawing`). A batch or an undo group gives one change set. A command that fails gives none. When you replace the whole document, you get `events.reset` instead: read the document again.

| Name | Use |
|---|---|
| `doc.events.subscribe(kinds=None, callback=None)` | Start the events. Returns the revision now. The callback gets each message. |
| `doc.events.take()` | The messages since the last call, oldest first. |
| `doc.events.wait(timeout)` | Like `take()`, and waits for a message (a connection to the app only). |
| `doc.events.poll(since_revision, kinds=None)` | The change sets after a revision, as `{revision, truncated, changes}`. For clients that cannot receive messages. If `truncated` is true, read the document again. |
| `doc.events.unsubscribe()` | Stop the events. |

Over a `kadakas serve` process, a message comes before the reply of the call that made the change. A connection to the app also gets the changes that the user makes, at any time. The change log has a limit; `poll` and `events.reset` tell you when changes were lost. MCP does not carry events.

### Everything else

The package has one method for every API method. They come from `describe`:

```python
doc.api.report_areas()
doc.api.hardscape_add(boundary=[[0, 0], [3000, 0], [3000, 1200], [0, 1200]])
doc.call("report.areas")        # the same, by the name of the method
doc.execute({"op": "add_layer", "name": "Notes"})   # any command
```

Errors raise `KadakasError`. It has `code`, `message` and `method`. A broken connection raises `ConnectionFailed`.

## Scripts palette

Choose **File > Scripts...** to open the palette.

1. Choose a folder with the **Choose...** button. The palette lists the `.py` files in it.
2. Click a file, then press **Run**.
3. The **Console** shows what the script prints. Errors are red.
4. **Stop** ends the script and everything it started.

The app sets two environment variables for the script: `KADAKAS_ADDR` and `KADAKAS_TOKEN`. In the script, call `kadakas.connect()` and you work on the open drawing. Each change is a normal command, so you can undo it.

```python
# scripts/number_circles.py
import kadakas

doc = kadakas.connect()
with doc.undo_group():
    for i, c in enumerate(doc.objects(kind="circle"), start=1):
        doc.add.text(c["center"], str(i), height=200)
print("numbered", i, "circles")
```

If the app has no agent server yet, the palette starts one on a free port the first time you press Run.

### Which Python is used

1. The program in `KADAKAS_PYTHON`. Use this to choose your own.
2. `python3` on PATH.
3. `uv run --no-config --no-project python`. This applies only when `uv` is on PATH. The app clears variables such as `UV_PYTHON`, `VIRTUAL_ENV` and `PYTHONHOME` first, and runs plug-in scripts in an empty private folder, so a planted `.python-version` or config file changes nothing.

The app puts the folder with the `kadakas` package on `PYTHONPATH`. It looks for `python/kadakas` next to the program, and in the folder where you started it. Set `KADAKAS_PYTHON_DIR` to point to it. You need none of this when you installed the package with `pip`.

## Plug-in objects

A plug-in object is a parametric object that you define yourself. It has:

- a **name**,
- a **shape type**: `point`, `linear`, `rectangle` or `path`,
- a list of **parameters**,
- a **Python script** that makes the shapes.

The definition is a resource in the drawing. You place objects from it. Each object keeps its own parameter values.

### Parameters

| Type | Value | Notes |
|---|---|---|
| `number` | a number | `min` and `max` are optional |
| `length` | a number in mm | `min` and `max` are optional |
| `text` | text | at most 10,000 characters |
| `bool` | true or false | |
| `choice` | one of `options` | |
| `color` | `"#rrggbb"` or `"#rrggbbaa"` | |
| `record` | an object with one entry per field | `fields` lists the fields, like a record format. A record cannot hold a record. |

Each parameter has a `name` (letters, digits and `_`), an optional `label`, and a `default`.

### The script

The script defines one function:

```python
def reset(params, geometry):
    return [circle(geometry.at, params.radius)]
```

The app calls `reset` each time a parameter or the geometry changes. This is the **reset event**.

`params` has one entry for each parameter. You can write `params.width` or `params["width"]`.

`geometry` holds the place of the object, in drawing coordinates (mm):

| Shape type | `geometry` |
|---|---|
| `point` | `at` (`[x, y]`), `rotation` (radians) |
| `linear` | `a`, `b` (two points) |
| `rectangle` | `min`, `max` (two corners) |
| `path` | `points` (a list of `[x, y]`), `closed` (true or false) |

`reset` returns a list of shapes. These helpers are ready to use, without an import:

| Helper | Shape or use |
|---|---|
| `line(a, b)` | a line |
| `rect(min, max)` | a rectangle |
| `circle(center, radius)` | a circle |
| `arc(center, radius, start, end)` | a counterclockwise arc |
| `polyline(points, closed=False)` | a polyline |
| `text(at, text, height)` | a text |
| `rotate(p, angle, about=(0, 0))` | turns a point |
| `walk(points, spacing, start=0, closed=False)` | gives `(x, y, angle)` along a path |
| `path_length(points, closed=False)` | the length of a path |
| `math` | the Python `math` module |

Every shape helper takes the optional arguments `pen`, `fill` and `weight`. Colors are `"#rrggbb"`.

`print()` in a script goes to the error output. It never breaks the answer.

### How it works

The app runs `reset` in a separate Python process. It sends one JSON message on the standard input and reads one JSON line from the standard output:

```json
{"event": "reset", "name": "Bench", "shape_type": "point",
 "params": {"length": 1800}, "geometry": {"at": [0, 0], "rotation": 0},
 "units": "mm", "script": "..."}
```

```json
{"ok": true, "shapes": [{"kind": "line", "a": [0, 0], "b": [1, 1]}]}
```

or `{"ok": false, "error": "...", "traceback": "..."}`.

The shapes are stored as the members of a group. Display, export (DXF, SVG, IFC) and Object Info never need Python. A drawing with plug-in objects opens on a computer with no Python. You only need Python to change parameters.

### Limits

| Limit | Value |
|---|---|
| Time for one reset | 5 seconds (change with `timeout_ms`, 10 to 60,000) |
| Shapes in one reset | 2,000 |
| Vertices in all polylines | 100,000 |
| Size of the answer | 2 MiB |
| Allowed shapes | `line`, `rect`, `circle`, `arc`, `polyline`, `text` |
| Circle and arc radius, text height | greater than zero |
| Text length | 10,000 characters |

If the script runs too long, it is stopped. If a limit is broken, the reset fails.

### What happens when the script fails

The app keeps the **shapes from the last good run**. The new parameter values are saved. The object shows an **error badge**: a red mark with an exclamation point on the canvas, and the error text in Object Info. Fix the parameter or the script, and press **Reset**.

### Moving an object

Move the object with the normal tools. The next reset follows the shapes to the new place. Turning a plug-in object with the group rotate command is not followed: change its `rotation` parameter or geometry instead.

### Object Info and undo

Select a plug-in object. **Object Info** shows its parameters. Change one, and the shapes are made again. Each change is one undo step. Placing an object is one undo step too.

### Shipped examples

| Name | Type | What it makes |
|---|---|---|
| **Bench** | point | A bench in plan view: seat, slats and an optional back rest. Parameters: length, depth, slats, back, material, color, supplier (a record). |
| **Stepping stones** | path | Stones along a path at an even spacing. Round or square, with an optional zigzag. Parameters: size, spacing, start_offset, zigzag, stone, color. |

Their scripts are in `python/examples/plugins/`. Add them to a drawing with `doc.plugins.install_example("Bench")`.

### Make your own

```python
import kadakas
from kadakas import Param

doc = kadakas.connect()

definition = doc.plugins.define(
    "Planter",
    "rectangle",
    params=[
        Param.length("wall", 150, min=50, max=500),
        Param.color("color", "#6b4f2a"),
        Param.bool("soil", True),
    ],
    script='''
def reset(params, geometry):
    lo, hi = geometry.min, geometry.max
    w = params.wall
    shapes = [rect(lo, hi, pen=params.color, fill=params.color)]
    if params.soil:
        shapes.append(rect([lo[0] + w, lo[1] + w], [hi[0] - w, hi[1] - w], fill="#3b2a14"))
    return shapes
''',
)

planter = doc.plugins.place(definition, min=[0, 0], max=[3000, 800])
planter.set_params(wall=250)
print(planter.params, planter.error)
```

To test a script without the app, use `kadakas.plugin.test_reset(script, params, geometry)`. It runs the script in your own Python and returns the shapes.

## Security

- **Scripts run only when a user starts them.** A drawing never runs code when you open it. The shapes of a plug-in object are stored, so they need no script.
- **Programs and AI agents cannot run untrusted scripts by default.** The calls `script.run` and `plugin.place` with a script of your own fail with the error code `4` (kind `policy`). The host must opt in. Start `kadakas serve`, `kadakas mcp` or the app with `--allow-agent-scripts`.
- **A script from a drawing never runs until you trust it.** A plug-in object script is a Python program. A drawing that you download may hold one. When you select such an object, Object Info shows a warning and the button **Review script...**. The dialog **Trust this plug-in?** shows the full source and where it comes from. Nothing runs until you press **Trust this script**.
- Trust is for the **exact text**. The app keeps the SHA-256 hash of each script you trusted in `trusted_plugins.json` in your config folder. If anyone changes the script, even by one character, the hash changes and the script is untrusted again. Putting the old text back makes it trusted again.
- The two shipped examples are trusted by their hash. Their scripts are part of the program.
- **Trust comes only from the dialog.** No API call adds trust. `plugin.update` can change a script, but the new text is untrusted. An agent cannot trust its own script.
- Opening a drawing, selecting an object and looking at Object Info never run a script. A parameter edit or **Reset** runs it only for a trusted script.
- The script of a reset runs in a separate thread. The window stays alive and shows a progress mark. A script that starts a background process does not hold the app: the app ends the whole process group when the reset ends or times out (on Windows only the script process is ended).
- `script.run` also needs the running app (it gives the script the address and token). A headless `kadakas serve` refuses it.
- A script is **not sandboxed**. It runs as you, with your files and network. Run only scripts that you trust. A plug-in object script runs with `python -I` (no user site folder, no `PYTHONPATH`). It also has the time and size limits above. These guard against mistakes. They do not stop a hostile script.
- A script file for an agent must be a `.py` file inside an allowed folder (`--allow-dir`).

## API reference

### Python package

| Name | Use |
|---|---|
| `kadakas.open(path=None, binary=None, allow_dir=(), allow_agent_scripts=False)` | Start `kadakas serve`. Returns a `Document`. |
| `kadakas.connect(addr=None, token=None, timeout=120)` | Join the running app. After a `RequestTimeout` the connection still works. |
| `doc.summary()`, `doc.describe()`, `doc.to_json()` | State of the session. |
| `doc.layers`, `doc.classes`, `doc.layer(x)`, `doc.class_(x)`, `doc.active_layer` | Read layers and classes. |
| `doc.add_layer(name, elevation)`, `doc.add_class(name, **attrs)` | Make them. |
| `doc.add.line / rect / circle / arc / polyline / text / shape(kind, ...)` | Draw. Optional `layer`, `class_`, `attrs`. |
| `doc.objects(layer, class_, kind, where)` (also `doc.query`), `doc.get(id)` | Query objects. |
| `Obj.id / kind / shape / record`, `Obj.modify(...)`, `.move(dx, dy)`, `.delete()`, `.set_record(**kw)`, `.refresh()` | One object. Shape fields are attributes: `circle.radius`. |
| `doc.move(items, dx, dy)`, `doc.delete(items)`, `doc.group(items)` | Many objects. |
| `doc.selection.ids() / objects() / set(items) / clear()` | The selection. |
| `doc.undo_group()`, `doc.undo()`, `doc.redo()` | Undo. |
| `doc.events.subscribe / take / wait / poll / unsubscribe` | Change events (see above). |
| `doc.plants.place / styles / create_style / schedule` | Plants. |
| `doc.report.plants / plant_schedule / areas / hardscape` | Reports. |
| `doc.worksheets.create / set_cell / evaluate / csv / place / list` | Worksheets. |
| `doc.import_file(path)`, `doc.export_file(path)`, `doc.save(path)`, `doc.open(path)` | Files. |
| `doc.render_png(path, ...)`, `doc.render_svg(path, width)` | Pictures. |
| `doc.plugins.examples / install_example / define / update / delete / list / get / place / object` | Plug-in objects. |
| `PluginObject.params / error / children / set_params(**kw) / reset()` | One plug-in object. |
| `doc.scripts.run / list / info`, `Run.status / wait / stop` | Scripts next to the app (needs the flag). |
| `doc.api.<method>(**params)`, `doc.call("a.b", **params)`, `doc.execute(command)` | Everything else. |
| `kadakas.plugin` | The shape helpers and `test_reset` for writing scripts. |

The text of every API method is the docstring of `doc.api.<method>`. Use `help(doc.api.plants_place)`.

### JSON-RPC methods for scripting

The methods `events.subscribe`, `events.unsubscribe` and `events.poll` give the change events. The server sends `{"jsonrpc": "2.0", "method": "events.change", "params": ChangeSet}` as a message without an id.


| Method | Use |
|---|---|
| `plugin.define`, `plugin.update`, `plugin.delete`, `plugin.list`, `plugin.get` | Definitions. |
| `plugin.examples`, `plugin.install_example` | The shipped examples. |
| `plugin.place`, `plugin.set_params`, `plugin.reset`, `plugin.object`, `plugin.preview` | Objects. |
| `script.info`, `script.run`, `script.status`, `script.stop`, `script.list` | Scripts next to the app. |

`describe` lists them with all parameters. See also `describe.scripting`.

### Command line flags

| Flag | Where | Use |
|---|---|---|
| `--allow-agent-scripts` | `kadakas serve`, `kadakas mcp`, the app | Let agents run scripts of their own. Default: off. |
| `--allow-converters` | `kadakas serve`, `kadakas mcp`, the app | Let agents start the DWG converter. Default: off. The file dialogs of the app are not limited. |
| `KADAKAS_PYTHON` | environment | The Python program to use. |
| `KADAKAS_PYTHON_DIR` | environment | The folder that holds the `kadakas` package. |
| `KADAKAS_BIN` | environment | The `kadakas` program, for the Python package. |
| `KADAKAS_SCRIPTS_DIR` | environment | The folder that the Scripts palette shows first. |

## Examples

- `python/examples/plugins/bench.py`: a point object.
- `python/examples/plugins/stepping_stones.py`: a path object.
- `python/tests/test_plugins.py`: shows every plug-in call, and runs both examples against a real session.

## Keep the package in step with the API

`python/kadakas/_api.py` is generated from the `describe` answer. Do not edit it by hand.

After you add or change an API method, run:

```sh
cargo build -p kadakas-cli
KADAKAS_BIN=target/debug/kadakas python python/tools/codegen.py
```

The test `python/tests/test_drift.py` fails in two cases. One: `_api.py` no longer matches `describe`. Two: a wrapper calls a method that does not exist.

Run the Python tests:

```sh
KADAKAS_BIN=target/debug/kadakas uv run --no-project --with pytest pytest python/tests
```
