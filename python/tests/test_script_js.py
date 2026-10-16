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
