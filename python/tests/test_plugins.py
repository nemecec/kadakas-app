"""Plug-in objects and scripts, through the Python package and a real ``kadakas serve``.

These are also the behavior tests for the two shipped examples (Bench, Stepping stones)."""

from __future__ import annotations

import math
from pathlib import Path

import pytest

import kadakas
from kadakas import KadakasError, Param
from kadakas.plugin import test_reset

EXAMPLES = Path(__file__).resolve().parents[1] / "examples" / "plugins"


def children_of(doc, plugin_object):
    return [doc.get(i) for i in plugin_object.children]


def test_the_examples_are_listed_and_installed_once(doc):
    names = [e["name"] for e in doc.plugins.examples()]
    assert names == ["Bench", "Stepping stones"]
    first = doc.plugins.install_example("Bench")
    assert doc.plugins.install_example("bench") == first
    assert [p["name"] for p in doc.plugins.list()] == ["Bench"]
    assert doc.plugins.get("Bench")["shipped"] is True
    assert {p["name"] for p in doc.plugins.get(first)["params"]} >= {"length", "slats", "supplier"}


def test_bench_example_draws_a_rotated_bench(doc):
    doc.plugins.install_example("Bench")
    bench = doc.plugins.place("Bench", at=[2000, 1000], rotation=math.pi / 2,
                              params={"length": 2000, "depth": 400, "slats": 4, "back": True})
    shapes = children_of(doc, bench)
    # Seat outline, 3 slat lines and the back rest.
    assert [s.kind for s in shapes] == ["polyline"] + ["line"] * 3 + ["polyline"]
    xs = [p[0] if isinstance(p, list) else p["p"][0] for p in shapes[0]["points"]]
    ys = [p[1] if isinstance(p, list) else p["p"][1] for p in shapes[0]["points"]]
    # Turned by 90 degrees: the long side (2000) runs along Y.
    assert max(ys) - min(ys) == pytest.approx(2000) and max(xs) - min(xs) == pytest.approx(400)
    assert bench.params["supplier"] == {"name": "", "article": ""}
    assert bench.error is None


def test_bench_parameters_change_the_shapes_and_undo_works(doc):
    doc.plugins.install_example("Bench")
    bench = doc.plugins.place("Bench", at=[0, 0])
    before = len(bench.children)
    res = bench.set_params(slats=9, back=False, supplier={"name": "Parks Ltd", "article": "B-7"})
    assert res["ok"] is True
    assert len(bench.children) == 9  # outline + 8 slat lines, no back rest
    assert bench.params["supplier"]["name"] == "Parks Ltd"
    assert doc.undo()
    assert len(bench.children) == before and bench.params["slats"] == 5
    with pytest.raises(KadakasError, match="range"):
        bench.set_params(slats=99)
    with pytest.raises(KadakasError, match="not one of"):
        bench.set_params(material="Gold")
    with pytest.raises(KadakasError, match="color"):
        bench.set_params(color="red")


def test_stepping_stones_example_follows_a_path(doc):
    doc.plugins.install_example("Stepping stones")
    path = [[0, 0], [3000, 0], [3000, 3000]]
    stones = doc.plugins.place("Stepping stones", points=path, params={"spacing": 1000, "size": 300})
    shapes = children_of(doc, stones)
    assert len(shapes) == 7  # path length 6000 mm, a stone every 1000 mm, both ends included
    assert all(s.kind == "circle" and s.radius == 150 for s in shapes)
    centers = [s["center"] for s in shapes]
    assert centers[0] == [0.0, 0.0] and centers[-1] == [3000.0, 3000.0]
    # Square stones, with a zigzag to both sides of the path.
    stones.set_params(stone="Square", zigzag=100)
    shapes = children_of(doc, stones)
    assert all(s.kind == "polyline" for s in shapes)
    first_y = [min(p[1] for p in s["points"]) for s in shapes[:3]]
    assert first_y[0] != first_y[1]
    # A closed path walks around once.
    ring = doc.plugins.place("Stepping stones", points=[[0, 0], [1000, 0], [1000, 1000], [0, 1000]],
                             closed=True, params={"spacing": 500})
    assert len(ring.children) == 9


def test_placing_is_one_undo_step_and_objects_follow_exports(doc, tmp_path: Path):
    doc.plugins.install_example("Bench")
    n = doc.summary()["object_count"]
    bench = doc.plugins.place("Bench", at=[0, 0])
    assert doc.summary()["object_count"] > n + 3
    # The shapes are real objects: export and IFC need no script.
    out = tmp_path / "plan.dxf"
    doc.export_file(out)
    assert b"LINE" in out.read_bytes() or b"LWPOLYLINE" in out.read_bytes()
    assert doc.undo()
    assert doc.summary()["object_count"] == n
    assert doc.redo()
    assert doc.plugins.object(bench.id).params["length"] == 1800


def test_a_script_error_keeps_the_old_shapes_and_shows_an_error(open_doc):
    doc = open_doc
    script = (
        "def reset(params, geometry):\n"
        "    if params.n > 4:\n"
        "        raise ValueError('too many')\n"
        "    return [circle(geometry.at, 10 * (i + 1)) for i in range(int(params.n))]\n"
    )
    d = doc.plugins.define("Rings", "point", script, [Param.number("n", 2, min=1, max=10)])
    rings = doc.plugins.place(d, at=[0, 0])
    assert len(rings.children) == 2
    res = rings.set_params(n=6)
    assert res["ok"] is False and "too many" in res["error"] and "ValueError" in res["traceback"]
    assert len(rings.children) == 2
    assert "too many" in rings.error and rings.params["n"] == 6
    assert rings.set_params(n=3)["ok"] is True and rings.error is None
    assert len(rings.children) == 3
    # A bad script is not placed at all.
    bad = doc.plugins.define("Bad", "point", "def reset(p, g):\n    return 1 / 0\n")
    before = doc.summary()["object_count"]
    with pytest.raises(KadakasError, match="ZeroDivisionError"):
        doc.plugins.place(bad, at=[0, 0])
    assert doc.summary()["object_count"] == before


def test_limits_stop_a_runaway_script(open_doc):
    doc = open_doc
    slow = doc.plugins.define("Slow", "point", "import time\ndef reset(p, g):\n    time.sleep(30)\n",
                              timeout_ms=300)
    with pytest.raises(KadakasError, match="300 ms"):
        doc.plugins.place(slow, at=[0, 0])
    many = doc.plugins.define("Many", "point",
                              "def reset(p, g):\n    return [circle([i, 0], 1) for i in range(3000)]\n")
    with pytest.raises(KadakasError, match="3000 shapes"):
        doc.plugins.place(many, at=[0, 0])
    nasty = doc.plugins.define("Nasty", "point",
                               "def reset(p, g):\n    return [{'kind': 'plant', 'at': [0, 0]}]\n")
    with pytest.raises(KadakasError, match="not allowed"):
        doc.plugins.place(nasty, at=[0, 0])


def test_agents_cannot_run_their_own_scripts_by_default(doc):
    d = doc.plugins.define("Mine", "point", "def reset(p, g):\n    return [circle(g.at, 5)]\n")
    with pytest.raises(KadakasError) as e:
        doc.plugins.place(d, at=[0, 0])
    assert e.value.is_forbidden and "--allow-agent-scripts" in str(e.value)
    with pytest.raises(KadakasError) as e:
        doc.scripts.run(source="print(1)")
    assert e.value.is_forbidden
    info = doc.scripts.info()
    assert info["agent_scripts_allowed"] is False and info["live_session"] is False
    # A headless session has no app address, so even an allowed agent cannot start a script.


def test_scripts_need_an_app_even_when_allowed(open_doc):
    assert open_doc.scripts.info()["agent_scripts_allowed"] is True
    with pytest.raises(KadakasError, match="running Kadakas app"):
        open_doc.scripts.run(source="print(1)")
    assert open_doc.scripts.list() == []


def test_definitions_update_and_regenerate_in_one_step(open_doc):
    doc = open_doc
    d = doc.plugins.define("Dot", "point", "def reset(p, g):\n    return [circle(g.at, 5)]\n",
                           description="one dot")
    a = doc.plugins.place("Dot", at=[0, 0])
    b = doc.plugins.place("Dot", at=[100, 0])
    assert doc.plugins.get("Dot")["instances"] == 2
    res = doc.plugins.update("Dot", script="def reset(p, g):\n    return [circle(g.at, 5), circle(g.at, 9)]\n",
                             reset=True)
    assert [r["ok"] for r in res["reset"]] == [True, True]
    assert len(a.children) == 2 and len(b.children) == 2
    doc.undo()
    assert len(a.children) == 1 and len(b.children) == 1
    with pytest.raises(KadakasError, match="cannot change"):
        doc.plugins.update(d, shape_type="path")
    with pytest.raises(KadakasError, match="objects use"):
        doc.plugins.delete("Dot")
    assert a.reset()["ok"] is True
    doc.delete(a.children + b.children)  # the groups go with their last members
    doc.plugins.delete("Dot")
    assert doc.plugins.list() == []


def test_a_document_with_plug_in_objects_saves_and_opens_again(doc, tmp_path: Path):
    doc.plugins.install_example("Bench")
    bench = doc.plugins.place("Bench", at=[10, 10])
    saved = tmp_path / "bench.kad"
    doc.save(saved)
    count = doc.summary()["object_count"]
    doc.undo()
    doc.open(saved)
    assert doc.summary()["object_count"] == count
    again = doc.plugins.object(bench.id)
    assert again.params["slats"] == 5 and again.error is None
    again.set_params(slats=6)
    assert len(again.children) == 7  # outline, 5 slat lines, back rest


def test_undo_group_refuses_plugin_place(doc):
    doc.plugins.install_example("Bench")
    with pytest.raises(KadakasError, match="undo group"):
        with doc.undo_group():
            doc.plugins.place("Bench", at=[0, 0])


def test_shipped_scripts_run_in_plain_python_too():
    """`kadakas.plugin.test_reset` runs a script without the app, for quick development."""
    bench = (EXAMPLES / "bench.py").read_text(encoding="utf-8")
    shapes = test_reset(bench, {"length": 1000, "depth": 400, "slats": 3, "back": False, "color": "#000000"},
                        {"at": [0, 0], "rotation": 0})
    assert len(shapes) == 3
    with pytest.raises(RuntimeError, match="NameError"):
        test_reset("def reset(p, g):\n    return [nothing]\n", {}, {})


def test_a_shipped_plugin_whose_script_was_changed_is_not_trusted(doc):
    """Changing the script of a definition through the API gives it a new hash: it does not run."""
    doc.plugins.install_example("Bench")
    doc.plugins.update("Bench", script="def reset(p, g):\n    return [circle(g.at, 5)]\n")
    with pytest.raises(KadakasError) as e:
        doc.plugins.place("Bench", at=[0, 0])
    assert e.value.is_forbidden
    assert doc.summary()["object_count"] == 0
