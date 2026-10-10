"""Plug-in objects and scripts through raw JSON-RPC. Needs `python3` (or `uv`) on PATH.

The shipped examples run without any flag. A script from an agent needs --allow-agent-scripts."""

import pytest

SCRIPT = "def reset(params, geometry):\n    return [circle(geometry.at, 10 * params.n)]\n"
DEFINITION = {
    "name": "Dot",
    "shape_type": "point",
    "script": SCRIPT,
    "params": [{"name": "n", "type": "number", "default": 1, "min": 1, "max": 5}],
}


def children(rpc, group):
    return [o for o in rpc("objects.list") if o.get("parent") == group]


def test_describe_lists_scripting(rpc):
    d = rpc("describe")["result"]
    names = {m["name"] for m in d["methods"]}
    assert {"plugin.define", "plugin.place", "plugin.set_params", "plugin.reset", "script.run"} <= names
    assert "reset(params, geometry)" in d["scripting"]["plugin_objects"]


def test_bench_example_runs_without_a_flag(cad):
    assert cad.result("plugin.install_example", {"name": "Bench"})["existing"] is False
    placed = cad.result("plugin.place", {"definition": "Bench", "at": [1000, 500], "rotation": 0.4})
    group = placed["id"]
    kids = children(cad.result, group)
    assert len(kids) == placed["shapes"] == 6
    assert {o["shape"]["kind"] for o in kids} == {"polyline", "line"}
    info = cad.result("plugin.object", {"id": group})
    assert info["params"]["length"] == 1800 and info["error"] is None


def test_stepping_stones_example_runs_without_a_flag(cad):
    cad.result("plugin.install_example", {"name": "Stepping stones"})
    placed = cad.result("plugin.place", {"definition": "Stepping stones", "points": [[0, 0], [2000, 0]],
                                         "params": {"spacing": 500, "size": 200}})
    assert placed["shapes"] == 5
    changed = cad.result("plugin.set_params", {"id": placed["id"], "params": {"spacing": 1000}})
    assert changed["ok"] and len(changed["children"]) == 3


def test_own_scripts_are_refused_without_the_flag(cad):
    cad.result("plugin.define", DEFINITION)
    err = cad.error("plugin.place", {"definition": "Dot", "at": [0, 0]})
    assert err["code"] == -32001 and "--allow-agent-scripts" in err["message"]
    assert cad.error("script.run", {"source": "print(1)"})["code"] == -32001
    assert cad.result("document.summary")["object_count"] == 0


def test_own_scripts_run_with_the_flag(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugin.define", DEFINITION)
    group = cad.result("plugin.place", {"definition": "Dot", "at": [0, 0], "params": {"n": 2}})["id"]
    assert children(cad.result, group)[0]["shape"]["radius"] == 20
    # A failing script keeps the old shapes and marks the object.
    cad.result("plugin.update", {"name": "Dot", "script": "def reset(p, g):\n    raise RuntimeError('broken')\n"})
    res = cad.result("plugin.set_params", {"id": group, "params": {"n": 3}})
    assert res["ok"] is False and "broken" in res["error"]
    assert children(cad.result, group)[0]["shape"]["radius"] == 20
    assert "broken" in cad.result("plugin.object", {"id": group})["error"]


def test_undo_takes_back_the_whole_placement(cad):
    cad.result("plugin.install_example", {"name": "Bench"})
    n = cad.result("document.summary")["object_count"]
    cad.result("plugin.place", {"definition": "Bench", "at": [0, 0]})
    assert cad.result("undo")["done"]
    assert cad.result("document.summary")["object_count"] == n


def test_bad_input_is_refused(cad):
    cad.result("plugin.install_example", {"name": "Bench"})
    assert cad.error("plugin.place", {"definition": "Bench", "at": [0, 0], "params": {"slats": 100}})["code"] == -32602
    assert cad.error("plugin.place", {"definition": "Nope", "at": [0, 0]})["code"] == 1
    assert cad.error("plugin.define", {"name": "X", "shape_type": "blob", "script": ""})["code"] == -32602
    assert cad.error("plugin.install_example", {"name": "Nope"})["code"] == -32602
    assert cad.error("script.status", {"run": 5})["code"] in (1, -32602)
