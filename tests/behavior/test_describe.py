"""Scenario: an agent starts by calling `describe` and tries the command examples it is given."""

import pytest

from helpers import JsonRpc

# Mirrors METHODS in crates/cadektor-core/src/api.rs.
ADVERTISED_METHODS = [
    "describe", "execute", "undo", "redo", "document.summary", "document.get", "document.load",
    "objects.list", "objects.get", "selection.get", "selection.set", "render.svg",
    "render.png", "view.list", "view.set", "report.plants", "report.areas",
    "plant_style.list", "plant_style.get", "plant_style.create", "plant_style.update", "plant_style.duplicate",
    "plant_style.delete", "plant_style.import_csv", "plant_style.export_csv", "plant_style.sample_csv",
    "plants.place", "plants.set_style", "report.plant_schedule", "report.existing_trees",
]


def test_describe_lists_every_method_with_a_description(cad):
    desc = cad.result("describe")
    names = [m["name"] for m in desc["methods"]]
    assert names == ADVERTISED_METHODS
    assert all(m["doc"] for m in desc["methods"])


def test_describe_states_units(cad):
    desc = cad.result("describe")
    assert "millimeters" in desc["units"]
    assert "Y axis up" in desc["units"]


@pytest.mark.parametrize("method", ADVERTISED_METHODS)
def test_every_advertised_method_is_known_to_the_server(cad, method):
    # Calling with no params is a param error at worst, never "method not found".
    resp = cad.rpc(method)
    if "error" in resp:
        assert resp["error"]["code"] != -32601, f"{method} is advertised but unknown"


def test_command_examples_run_in_order_in_a_fresh_session(make_serve):
    cad: JsonRpc = make_serve()
    examples = cad.result("describe")["command_examples"]
    assert examples
    for example in examples:
        resp = cad.rpc("execute", {"command": example})
        if "error" in resp:
            # Some examples name object ids that may not exist; only the error type is checked.
            assert resp["error"]["code"] == 1, resp
            assert isinstance(resp["error"]["message"], str)
        else:
            assert "created" in resp["result"]
    # The session stays healthy after the examples.
    assert cad.result("document.summary")["object_count"] >= 1
