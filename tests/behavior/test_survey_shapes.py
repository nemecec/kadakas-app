"""Scenario: 3D survey shapes keep their Z values, move in XY only and render in 2D."""

from pytest import approx

from helpers import parse_svg


def test_locus3d_and_polyline3d_keep_z_and_move_in_xy_only(cad):
    (locus,) = cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [1000, 2000, 350]}})
    (contour,) = cad.execute({"op": "add_object", "shape": {
        "kind": "polyline3d", "closed": True, "points": [[0, 0, 10], [4000, 0, 20], [4000, 3000, 30]]}})
    cad.execute({"op": "move_objects", "ids": [locus, contour], "delta": [100, -50]})
    assert cad.result("objects.get", {"id": locus})["shape"]["p"] == [1100, 1950, 350]
    shape = cad.result("objects.get", {"id": contour})["shape"]
    assert shape["kind"] == "polyline3d"
    assert shape["points"] == [[100, -50, 10], [4100, -50, 20], [4100, 2950, 30]]
    svg = cad.result("render.svg", {"width": 400})
    assert 'data-kind="locus3d"' in str(svg) and "<polygon" in str(svg)
    parse_svg(svg if isinstance(svg, str) else svg["svg"])


def test_polyline3d_needs_two_points_and_finite_in_range_z(cad):
    one = {"kind": "polyline3d", "closed": False, "points": [[0, 0, 0]]}
    assert "2 or more" in cad.error("execute", {"command": {"op": "add_object", "shape": one}})["message"]
    far = {"kind": "locus3d", "p": [0, 0, 1e10]}
    assert "1e9" in cad.error("execute", {"command": {"op": "add_object", "shape": far}})["message"]
