"""Scenario: a landscape architect joins a planter and a turned bench with the solid tools. A
corner of the bench lies 0.0058 mm off the face of the planter. The tool refuses and names the
vertex. The agent allows the snap, the tool works, and one undo takes both back."""

import math

import pytest

OPS = ["union", "subtract", "intersect", "trim", "split"]


def cuboid(lo, hi):
    def corner(i):
        return [hi[k] if i >> k & 1 else lo[k] for k in range(3)]

    return [[corner(i), corner(i | bit)] for i in range(8) for bit in (1, 2, 4) if not i & bit]


def turned(angle, tilt, centre):
    """The edges of a box of 800 mm turned about the vertical axis and tilted."""
    sin, cos = math.sin(angle), math.cos(angle)
    st, ct = math.sin(tilt), math.cos(tilt)
    corners = []
    for i in range(8):
        x, y, z = (400.0 if i >> k & 1 else -400.0 for k in range(3))
        zx, zy = x * cos - y * sin, x * sin + y * cos
        ty, tz = zy * ct - z * st, zy * st + z * ct
        corners.append([zx + centre[0], ty + centre[1], tz + centre[2]])
    return [[corners[i], corners[i | bit]] for i in range(8) for bit in (1, 2, 4) if not i & bit]


def near_touch(cad):
    """The box [0, 1000]^3 and a turned box with a corner 0.0058 mm off the plane y = 1000."""
    model = cad.result("mesh.create", {"name": "Planter and bench"})["id"]
    cad.result("mesh.add_edges", {"id": model, "edges": cuboid([0, 0, 0], [1000, 1000, 1000])})
    cad.result("mesh.add_edges", {
        "id": model, "edges": turned(1.5396231483638576, 0.3945730627897836, [200.0, 796.1958056970052, 200.0])})
    report = cad.result("mesh.solid_report", {"id": model})
    inward = [f for s in report["shells"] if s["state"] == "inverted" for f in s["faces"]]
    if inward:
        cad.result("mesh.reverse_faces", {"id": model, "faces": inward})
    shells = cad.result("mesh.solid_report", {"id": model})["shells"]
    assert [s["state"] for s in shells] == ["solid", "solid"]
    return model, shells[0]["faces"][0], shells[1]["faces"][0]


def geometry(cad, model):
    return [cad.result(m, {"id": model, "limit": 1000})["items"] for m in ("mesh.vertices", "mesh.edges", "mesh.faces")]


@pytest.mark.parametrize("op", OPS)
def test_the_refusal_names_the_vertex_and_the_flag_and_changes_nothing(cad, op):
    model, a, b = near_touch(cad)
    before = geometry(cad, model)
    err = cad.error("mesh.solid", {"id": model, "op": op, "first_face": a, "second_face": b})
    assert err["data"]["error_id"] == "near_touch_snap_available"
    assert err["data"]["flag"] == "snap_near"
    [vertex] = err["data"]["vertices"]
    assert vertex["distance"] == pytest.approx(0.0058, abs=1e-4)
    assert vertex["to"][1] == pytest.approx(1000.0)
    assert "lies 0.0058 mm from the plane" in err["message"] and "snap_near" in err["message"]
    assert geometry(cad, model) == before


@pytest.mark.parametrize("op", OPS)
def test_snap_near_works_lists_the_vertex_and_one_undo_takes_it_back(cad, op):
    model, a, b = near_touch(cad)
    before = geometry(cad, model)
    result = cad.result("mesh.solid", {"id": model, "op": op, "first_face": a, "second_face": b, "snap_near": True})
    [moved] = result["snapped"]
    assert moved["distance"] == pytest.approx(0.0058, abs=1e-4)
    assert moved["from"][1] == pytest.approx(1000.0058, abs=1e-4)
    assert moved["to"][1] == pytest.approx(1000.0)
    assert any("snapped vertex" in n for n in result["notes"])
    assert cad.result("mesh.solid_report", {"id": model})["closed"]
    assert cad.result("undo") == {"done": True}
    assert geometry(cad, model) == before


def test_a_model_without_a_near_touch_is_the_same_with_the_flag(cad):
    results = []
    for flag in (False, True):
        model = cad.result("mesh.create", {"name": "Boxes"})["id"]
        for lo, hi in (([0, 0, 0], [1000, 1000, 1000]), ([500, 500, 500], [1500, 1500, 1500])):
            cad.result("mesh.add_edges", {"id": model, "edges": cuboid(lo, hi)})
        report = cad.result("mesh.solid_report", {"id": model})
        inward = [f for s in report["shells"] if s["state"] == "inverted" for f in s["faces"]]
        if inward:
            cad.result("mesh.reverse_faces", {"id": model, "faces": inward})
        shells = cad.result("mesh.solid_report", {"id": model})["shells"]
        params = {"id": model, "op": "union", "first_face": shells[0]["faces"][0], "second_face": shells[1]["faces"][0]}
        if flag:
            params["snap_near"] = True
        result = cad.result("mesh.solid", params)
        assert result.pop("snapped", []) == []
        results.append((result["counts"], cad.result("mesh.solid_report", {"id": model})["shells"][0]["volume"]))
    assert results[0] == results[1]
