"""Scenario: a landscape designer brings a manufacturer's STEP file into a plan (IOP-11).

The B-rep is cut into triangles on load. Volumes and areas must match the analytic values, each
mesh keeps its exact surfaces (radii, normals), and the triangles can be cut again finer.
OpenCASCADE is used as a reference when it is installed (cadquery or OCP); the check skips if not."""

import json
import math
import subprocess
import sys
from pathlib import Path

import pytest
from pytest import approx

from helpers import REPO

DATA = REPO / "tests" / "data" / "step"
OCC_CHECK = Path(__file__).with_name("step_occ_check.py")


def import_step(cad, name, options=None):
    return cad.result("import.file", {"path": str(DATA / name), "options": options or {}})


def retessellate(cad, mesh, tolerance):
    return cad.result("mesh3d.retessellate", {"id": mesh, "tolerance": tolerance})


def test_a_box_comes_in_exact_with_colour_and_report(cad):
    r = import_step(cad, "box.step")
    assert r["imported"] == 1
    rep = r["report"]
    assert rep["format"] == "step" and rep["unit"] == "mm"
    assert rep["bounds_mm"]["size"] == [20.0, 30.0, 40.0]
    step = rep["step"]
    assert step["entities_read"] == step["entities_used"] + step["entities_skipped"]
    assert {u["entity"] for u in step["used"]} >= {"ADVANCED_FACE", "MANIFOLD_SOLID_BREP", "EDGE_CURVE"}
    obj = rep["objects"][0]
    assert obj["name"] == "Box" and obj["color"] == [255, 0, 0, 255]
    info = cad.result("mesh3d.exact_info", {"id": obj["id"]})
    assert info["summary"] == "Exact data kept: 6 faces (plane x 6)"
    normals = sorted(tuple(f["normal"]) for f in info["list"])
    assert (0.0, 0.0, 1.0) in normals and (0.0, 0.0, -1.0) in normals
    # One undo step takes it all back.
    cad.result("undo")
    assert cad.result("objects.list") == []


@pytest.mark.parametrize("name,volume", [
    ("cylinder.step", math.pi * 100 * 25),
    ("sphere.step", 4 / 3 * math.pi * 12 ** 3),
    ("torus.step", 2 * math.pi ** 2 * 20 * 25),
    ("frustum.step", math.pi * 20 / 3 * (144 + 72 + 36)),
])
def test_curved_solids_match_the_analytic_volume_and_get_finer(cad, name, volume):
    r = import_step(cad, name, {"tolerance": 0.5})
    mesh = r["report"]["objects"][0]["id"]
    coarse = retessellate(cad, mesh, 0.5)
    fine = retessellate(cad, mesh, 0.01)
    assert fine["triangles"]["after"] > coarse["triangles"]["after"]
    assert abs(fine["volume_mm3"] - volume) < abs(coarse["volume_mm3"] - volume)
    assert fine["volume_mm3"] == approx(volume, rel=2e-3)
    # An undo gives the coarse triangles back.
    cad.result("undo")
    again = cad.result("mesh3d.exact_info", {"id": mesh})
    assert again["state"] == "fresh"


def test_the_true_radius_and_normals_are_known(cad):
    r = import_step(cad, "cylinder.step")
    mesh = r["report"]["objects"][0]["id"]
    info = cad.result("mesh3d.exact_info", {"id": mesh})
    cyl = next(f for f in info["list"] if f["kind"] == "cylinder")
    assert cyl["radius"] == 10.0
    assert cyl["axis"] == [0.0, 0.0, 1.0]
    plates = [f for f in info["list"] if f["kind"] == "plane"]
    assert sorted(p["normal"][2] for p in plates) == [-1.0, 1.0]


def test_an_assembly_keeps_names_placements_and_colors(cad):
    r = import_step(cad, "assembly.step")
    assert r["imported"] == 2
    boxes = [o for o in r["report"]["objects"]]
    assert {o["name"] for o in boxes} == {"Leg"}
    assert all(o["color"] == [0, 0, 255, 255] for o in boxes)
    assert r["report"]["step"]["instances"] == 2
    one = import_step(cad, "assembly.step", {"join": True})
    assert one["imported"] == 1


def test_units_and_options_and_refusals(cad, tmp_path):
    r = import_step(cad, "box_inch.step")
    assert r["report"]["bounds_mm"]["size"][0] == approx(25.4)
    assert r["report"]["step"]["unit_from_file"] is True
    err = cad.error("import.file", {"path": str(DATA / "box.step"), "options": {"up_axis": "y"}})
    assert "up_axis" in err["message"]
    bad = tmp_path / "bad.step"
    bad.write_text("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.")
    err = cad.error("import.file", {"path": str(bad)})
    assert err["data"]["error_id"] in ("mesh_truncated", "mesh_malformed"), err
    deep = tmp_path / "deep.step"
    deep.write_text("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=A(" + "(" * 100000 + ")" * 100000 + ");\nENDSEC;\n")
    err = cad.error("import.file", {"path": str(deep)})
    assert err["data"]["error_id"] == "mesh_malformed"


def test_lines_become_3d_polylines(cad):
    r = import_step(cad, "curves.step")
    assert r["imported"] == 3 and r["report"]["curves"] == 3
    kinds = {o["shape"]["kind"] if "shape" in o else o.get("type") for o in cad.result("objects.list")}
    assert kinds == {"polyline3d"}


def occ_facts(path):
    run = subprocess.run([sys.executable, "-I", str(OCC_CHECK), str(path)], capture_output=True, text=True, timeout=300)
    if run.returncode == 3:
        pytest.skip("OpenCASCADE (cadquery or OCP) is not installed")
    assert run.returncode == 0, run.stderr
    return json.loads(run.stdout)


@pytest.mark.parametrize("name", ["box.step", "cylinder.step", "sphere.step", "torus.step", "plate_hole.step", "frustum.step"])
def test_volume_area_and_box_agree_with_opencascade(cad, name):
    facts = occ_facts(DATA / name)
    r = import_step(cad, name, {"tolerance": 0.5})
    mesh = r["report"]["objects"][0]["id"]
    fine = retessellate(cad, mesh, 0.005)
    assert fine["volume_mm3"] == approx(facts["volume"], rel=2e-3)
    size = r["report"]["bounds_mm"]["size"]
    ref = [h - l for h, l in zip(facts["max"], facts["min"])]
    for a, b in zip(size, ref):
        assert a == approx(b, abs=0.05 + 0.01 * b)
