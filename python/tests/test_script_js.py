"""JavaScript programs inside the session (SCR-08), through the Python package and a real
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


def test_read_mode_allows_read_only_methods_only_and_refuses_before_the_call_runs(doc):
    code = "kadakas.call('execute', {command: {op: 'add_layer', name: 'X'}});"
    with pytest.raises(KadakasError) as e:
        doc.script_eval(code, mode="read")
    assert "read-only" in str(e.value)
    assert len(doc.api.document_summary()["layers"]) == 1
    r = doc.script_eval("return kadakas.call('document.summary', {}).layers.length;", mode="read")
    assert r["result"] == 1
    doc.script_eval(code)
    assert len(doc.api.document_summary()["layers"]) == 2


def test_the_same_script_gives_the_same_result(doc):
    code = "const a = []; for (let i = 0; i < 3; i++) a.push(Math.random()); return a;"
    assert doc.script_eval(code, seed=5)["result"] == doc.script_eval(code, seed=5)["result"]
    assert doc.script_eval(code, seed=5)["result"] != doc.script_eval(code, seed=6)["result"]


def test_some_methods_are_never_called_by_a_script(doc):
    for method in ("script.eval", "undo", "document.load", "plugin.place", "collab.connect"):
        with pytest.raises(KadakasError) as e:
            doc.script_eval(f"kadakas.call('{method}', {{}});")
        assert "cannot call" in str(e.value), method


GRID = """
const b = args.box, w = args.w, h = args.h;
const cols = Math.ceil((b[2] - b[0]) / w), rows = Math.ceil((b[3] - b[1]) / h);
const data = new Float64Array(cols * rows * 5), kind = new Uint32Array(cols * rows);
let n = 0;
for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) {
  data.set([b[0] + c * w + w / 2, b[1] + r * h + h / 2, w, h, 0], n * 5);
  kind[n++] = r % 2;
}
"""


def test_a_program_makes_a_generator_result_and_the_document_stores_it(doc):
    """1000 x 250 mm, units of 200 x 100 mm: five columns, two whole rows and a third row cut at
    half (hand-checked: 15 units, 10 whole, 5 cut of half a unit). The program stores its result
    in a hardscape style; after that no code runs."""
    r = doc.script_eval(
        GRID
        + """
        kadakas.call("hardscape_style.create", {name: "Generated", joints: {kind: "generator", spacing: 0,
          generator: {label: "grid", params: args, units: {types: ["even", "odd"], rects: {kind, data}}}}});
        return kadakas.call("hardscape.add", {boundary: [[0, 0], [1000, 0], [1000, 250], [0, 250]],
          style: "Generated"}).id;
        """,
        args={"box": [0, 0, 1000, 250], "w": 200, "h": 100},
    )
    assert r["undo_steps"] == 1
    report = doc.report.unit_pattern(ids=[r["result"]])
    row = report["rows"][0]
    assert (row["whole_units"], row["cut_units"], row["order_units"]) == (10, 5, 15)
    assert [(u["name"], u["whole_units"], u["cut_units"]) for u in row["by_unit"]] == [
        ("even", 5, 5), ("odd", 5, 0)]
    assert abs(row["area_laid_m2"] - 0.25) < 1e-9


def test_a_generator_pattern_in_a_layout_call_and_as_objects(doc):
    pattern = {"kind": "generator", "label": "two", "types": ["a"], "gap": 0,
               "rects": {"data": [100, 50, 200, 100, 0, 300, 50, 200, 100, 0]}}
    layout = doc.pattern_layout(
        region={"outline": [[0, 0], [400, 0], [400, 100], [0, 100]]}, pattern=pattern, origin=[0, 0])
    assert layout["units_total"] == 2 and layout["summary"]["whole_units"] == 2
    outline = doc.add.polyline([[0, 0], [400, 0], [400, 100], [0, 100]], closed=True)
    made = doc.pattern_to_objects(id=outline.id, pattern=pattern, origin=[0, 0])
    assert made["units"] == 2
    with pytest.raises(KadakasError) as e:
        doc.pattern_layout(region={"outline": [[0, 0], [10, 0], [10, 10], [0, 10]]},
                           pattern={"kind": "generator", "gap": 0}, origin=[0, 0])
    assert "no stored result" in str(e.value)
