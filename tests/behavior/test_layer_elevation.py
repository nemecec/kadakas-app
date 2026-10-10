"""Scenario: DOC-11. A design layer has an elevation, and 3D heights are measured from it."""

import pytest


SQUARE = [[0, 0], [5000, 0], [5000, 5000], [0, 5000]]


def massing_base(cad, layer):
    mid = cad.result("massing.add", {"footprint": SQUARE, "height": 3000, "layer": layer})["id"]
    row = cad.result("massing.report", {"ids": [mid]})["rows"][0]
    return mid, row["base_elevation_m"], row["eave_height_m"]


def test_a_layer_has_an_elevation_that_the_3d_model_uses(cad):
    ground = cad.execute({"op": "add_layer", "name": "Ground", "elevation": 0})[0]
    terrace = cad.execute({"op": "add_layer", "name": "Terrace", "elevation": 2500})[0]
    layers = {l["id"]: l for l in cad.result("document.summary")["layers"]}
    assert layers[terrace]["elevation"] == 2500
    _, base0, eave0 = massing_base(cad, ground)
    _, base1, eave1 = massing_base(cad, terrace)
    assert base0 == 0
    assert base1 == pytest.approx(2.5)
    # The height of the block is the same on both layers: it is measured from the base.
    assert eave0 == eave1 == pytest.approx(3.0)


def test_changing_the_elevation_moves_what_stands_on_the_layer_and_undo_restores_it(cad):
    layer = cad.execute({"op": "add_layer", "name": "Terrace", "elevation": 1000})[0]
    mid, base, _ = massing_base(cad, layer)
    assert base == pytest.approx(1.0)
    cad.execute({"op": "modify_layer", "id": layer, "elevation": -2000})
    assert cad.result("massing.report", {"ids": [mid]})["rows"][0]["base_elevation_m"] == pytest.approx(-2.0)
    cad.result("undo")
    assert cad.result("massing.report", {"ids": [mid]})["rows"][0]["base_elevation_m"] == pytest.approx(1.0)


def test_elevations_beyond_the_limit_are_refused(cad):
    resp = cad.rpc("execute", {"command": {"op": "add_layer", "name": "Bad", "elevation": 1e12}})
    assert resp["error"]["code"] == 1 and "1e9" in resp["error"]["message"]
    layer = cad.execute({"op": "add_layer", "name": "Ok", "elevation": 5})[0]
    resp = cad.rpc("execute", {"command": {"op": "modify_layer", "id": layer, "elevation": -1e12}})
    assert resp["error"]["code"] == 1
    assert [l["elevation"] for l in cad.result("document.summary")["layers"]][-1] == 5
