"""Scenario: curved polylines. Rounded outlines have an exact area, and old files still load."""

import math

import pytest
from pytest import approx

from helpers import parse_svg


def arc(x, y, radius):
    return {"p": [x, y], "kind": "arc", "radius": radius}


def rounded_square(side, radius):
    return [arc(0, 0, radius), arc(side, 0, radius), arc(side, side, radius), arc(0, side, radius)]


def analytic_area_m2(side, radius):
    return (side * side - (4 - math.pi) * radius * radius) / 1e6


def test_rounded_corner_hardscape_area_matches_the_analytic_value(cad):
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": rounded_square(4000, 500),
                                               "material": "Setts"}})
    (row,) = cad.result("report.areas")["rows"]
    assert row["area_m2"] == approx(analytic_area_m2(4000, 500), rel=1e-6)
    assert row["area_m2"] < 16.0, "rounding the corners takes area away"


@pytest.mark.parametrize(("side", "radius"), [(1000, 100), (3000, 1500), (2500, 0)])
def test_area_for_several_radii(cad, side, radius):
    cad.execute({"op": "add_object", "shape": {"kind": "landscape_area", "boundary": rounded_square(side, radius),
                                               "name": "Bed"}})
    (row,) = cad.result("report.areas")["rows"]
    assert row["area_m2"] == approx(analytic_area_m2(side, radius), rel=1e-6)


def test_too_large_radius_is_clamped_to_fit(cad):
    # A radius larger than half the side makes a circle. It is not an error.
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": rounded_square(2000, 99999),
                                               "material": "Gravel"}})
    (row,) = cad.result("report.areas")["rows"]
    assert row["area_m2"] == approx(math.pi * 1000 * 1000 / 1e6, rel=1e-6)


def test_plain_points_still_work_and_corners_are_saved_as_plain_points(cad):
    obj_id = cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": False,
                                                       "points": [[0, 0], arc(100, 0, 20), [100, 100]]}})[0]
    shape = cad.result("objects.get", {"id": obj_id})["shape"]
    assert shape["points"][0] == [0, 0]
    assert shape["points"][1] == {"p": [100, 0], "kind": "arc", "radius": 20}
    assert shape["points"][2] == [100, 100]


def test_set_vertex_changes_a_vertex_and_undo_restores_it(cad):
    obj_id = cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": rounded_square(2000, 100),
                                                       "material": "Setts"}})[0]
    before = cad.result("report.areas")["rows"][0]["area_m2"]
    cad.execute({"op": "set_vertex", "id": obj_id, "index": 1, "kind": {"kind": "arc", "radius": 400}})
    cad.execute({"op": "set_vertex", "id": obj_id, "index": 2, "p": [2200, 2000]})
    after = cad.result("report.areas")["rows"][0]["area_m2"]
    assert after != approx(before)
    boundary = cad.result("objects.get", {"id": obj_id})["shape"]["boundary"]
    assert boundary[1]["radius"] == 400
    assert boundary[2]["p"] == [2200, 2000]
    cad.result("undo")
    cad.result("undo")
    assert cad.result("report.areas")["rows"][0]["area_m2"] == approx(before)


@pytest.mark.parametrize("command", [
    {"op": "set_vertex", "id": 2, "index": 9, "kind": {"kind": "corner"}},
    {"op": "set_vertex", "id": 2, "index": 0, "kind": {"kind": "arc", "radius": -5}},
    {"op": "set_vertex", "id": 99, "index": 0, "p": [0, 0]},
])
def test_set_vertex_errors_are_reported_and_change_nothing(cad, command):
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": rounded_square(2000, 100),
                                               "material": "Setts"}})
    before = cad.doc()
    reply = cad.rpc("execute", {"command": command})
    assert "error" in reply
    assert cad.doc() == before


def test_negative_radius_is_rejected_when_adding(cad):
    reply = cad.rpc("execute", {"command": {"op": "add_object", "shape": {
        "kind": "polyline", "closed": False, "points": [[0, 0], arc(10, 0, -1), [10, 10]]}}})
    assert reply["error"]["message"] == "invalid value: vertex radius must be zero or more"


def test_svg_uses_true_arc_commands_for_curves(cad):
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": rounded_square(1000, 100),
                                               "material": "Setts"}})
    cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": False, "points": [
        [2000, 0], {"p": [2500, 500], "kind": "bezier"}, [3000, 0], {"p": [3300, 400], "kind": "cubic"},
        {"p": [3600, 400], "kind": "cubic"}, [4000, 0]]}})
    svg = cad.result("render.svg")["svg"]
    parse_svg(svg)  # well-formed XML
    assert " A 100 100 0 0 0 " in svg
    assert " Q " in svg and " C " in svg


def test_old_files_with_plain_points_load(cad):
    old = cad.doc()
    old["objects"] = [{"id": 2, "layer": 1, "class": 0, "attrs": {},
                       "shape": {"kind": "hardscape", "boundary": [[0, 0], [3000, 0], [3000, 1200], [0, 1200]],
                                 "material": "Granite setts"}}]
    old["next_id"] = 3
    cad.result("document.load", {"document": old})
    (row,) = cad.result("report.areas")["rows"]
    assert row["area_m2"] == approx(3.6)


def test_describe_shows_a_filleted_polyline_example(cad):
    examples = cad.result("describe")["command_examples"]
    assert any(e.get("op") == "set_vertex" for e in examples)
    assert any(e.get("shape", {}).get("kind") == "polyline" and any(isinstance(p, dict) for p in e["shape"]["points"])
               for e in examples if e.get("op") == "add_object")
