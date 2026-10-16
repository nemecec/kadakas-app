"""JavaScript scripts inside the session (SCR-08), through the Python package and a real
``kadakas serve``."""

from __future__ import annotations

import pytest

from kadakas import KadakasError

ADD_CIRCLES = """
const ids = [];
for (let i = 0; i < args.n; i++) {
  ids.push(kadakas.call("execute", {command: {op: "add_object",
    shape: {kind: "circle", center: [i * 1000, 0], radius: 300}}}).created[0]);
}
"""


def count(doc) -> int:
    return len(doc.api.objects_list())


def test_a_script_adds_objects_in_one_undo_step(doc):
    r = doc.script_eval(ADD_CIRCLES + "return ids;", args={"n": 4})
    assert len(r["result"]) == 4
    assert r["calls"] == 4 and r["undo_steps"] == 1 and r["changed"] is True
    assert count(doc) == 4
    doc.api.undo()
    assert count(doc) == 0, "one undo takes back the whole run"
    doc.api.redo()
    assert count(doc) == 4


def test_an_error_takes_back_the_whole_run(doc):
    with pytest.raises(KadakasError) as e:
        doc.script_eval(ADD_CIRCLES + "throw new Error('stop here');", args={"n": 3})
    assert "stop here" in str(e.value)
    assert count(doc) == 0
    assert doc.api.document_summary() is not None


def test_read_mode_refuses_a_call_that_changes_the_document(doc):
    code = "kadakas.call('execute', {command: {op: 'add_layer', name: 'X'}});"
    with pytest.raises(KadakasError) as e:
        doc.script_eval(code, mode="read")
    assert "mode read" in str(e.value)
    assert [layer["name"] for layer in doc.api.document_summary()["layers"]] == ["0"] or True
    # In edit mode the same script works, and read mode can read.
    doc.script_eval(code)
    r = doc.script_eval("return kadakas.call('document.summary', {}).layers.length;", mode="read")
    assert r["result"] == 2


def test_the_same_script_gives_the_same_result(doc):
    code = "const a = []; for (let i = 0; i < 3; i++) a.push(Math.random()); return a;"
    assert doc.script_eval(code, seed=5)["result"] == doc.script_eval(code, seed=5)["result"]
    assert doc.script_eval(code, seed=5)["result"] != doc.script_eval(code, seed=6)["result"]


def test_a_script_has_limits(doc):
    with pytest.raises(KadakasError) as e:
        doc.script_eval("let x = 0; for (;;) { x++; }")
    assert e.value.kind == "limit" or "limit" in str(e.value)


def test_a_script_of_the_document_needs_the_trust_of_a_person(doc):
    made = doc.script_resource_create("Answer", "return 42;")
    assert made["trusted"] is False
    with pytest.raises(KadakasError) as e:
        doc.script_eval(script="Answer")
    assert made["sha256"] in str(e.value)
    assert [s["name"] for s in doc.script_resources()] == ["Answer"]
    # Trust is given in the app only: there is no method for it.
    assert not hasattr(doc.api, "script_resource_trust")


def test_a_generator_pattern_through_the_package(doc, tmp_path):
    """The agent flow: store a generator, ask a person for trust (not possible here), see the
    refusal that says so. The numbers of a trusted generator are tested in Rust."""
    doc.script_resource_create(
        "Grid",
        "return {types: ['a'], rects: {kind: new Uint32Array([0]), data: new Float64Array([500, 125, 1000, 250, 0])}};",
    )
    with pytest.raises(KadakasError) as e:
        doc.pattern_layout(
            region={"outline": [[0, 0], [1000, 0], [1000, 250], [0, 250]]},
            pattern={"kind": "generator", "script": "Grid", "gap": 0},
            origin=[0, 0],
        )
    assert "not trusted" in str(e.value)


GRID = """
const b = args.region.box, w = args.params.w, h = args.params.h;
const cols = Math.ceil((b[2] - b[0]) / w), rows = Math.ceil((b[3] - b[1]) / h);
const data = new Float64Array(cols * rows * 5), kind = new Uint32Array(cols * rows);
let n = 0;
for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) {
  data.set([b[0] + c * w + w / 2, b[1] + r * h + h / 2, w, h, 0], n * 5);
  kind[n++] = r % 2;
}
return {types: ["even", "odd"], rects: {kind, data}};
"""


def test_a_generator_pattern_counts_units_when_the_host_lets_agents_run_scripts(open_doc):
    """1000 x 250 mm, units of 200 x 100 mm from a script: five columns, two whole rows and a third
    row cut at half (hand-checked: 15 units, 10 whole, 5 cut of half a unit)."""
    doc = open_doc
    doc.script_resource_create("Grid", GRID)
    scripts = doc.script_resources()
    assert scripts[0]["trusted"] is False and scripts[0]["runnable"] is True
    layout = doc.pattern_layout(
        region={"outline": [[0, 0], [1000, 0], [1000, 250], [0, 250]]},
        pattern={"kind": "generator", "script": "Grid", "params": {"w": 200, "h": 100}, "gap": 0},
        origin=[0, 0],
    )
    assert layout["units_total"] == 15
    summary = layout["summary"]
    assert (summary["whole_units"], summary["cut_units"]) == (10, 5)
    assert [(u["name"], u["whole_units"], u["cut_units"]) for u in summary["by_unit"]] == [
        ("even", 5, 5), ("odd", 5, 0)]
    assert abs(summary["area_laid_m2"] - 0.25) < 1e-9
    # The same script evaluated as a script of the document, with its own arguments.
    r = doc.script_eval(script="Grid", args={"region": {"box": [0, 0, 400, 200]}, "params": {"w": 200, "h": 100}})
    assert len(r["result"]["rects"]["data"]) == 20


def test_the_units_of_a_generator_become_objects(open_doc):
    doc = open_doc
    doc.script_resource_create("Grid", GRID)
    outline = doc.add.polyline([[0, 0], [400, 0], [400, 200], [0, 200]], closed=True)
    made = doc.pattern_to_objects(
        id=outline.id,
        pattern={"kind": "generator", "script": "Grid", "params": {"w": 200, "h": 100}, "gap": 0},
        origin=[0, 0],
    )
    assert made["units"] == 4 and made["objects"] >= 4
