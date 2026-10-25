"""Scenario: the cuts and the solids of IFC files from other programs (IOP-29). IfcOpenShell is the independent
reference: it writes the files, and it computes the volumes that Kadakas must get. Skipped when `uv` or
IfcOpenShell is not available."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from pytest import approx

from helpers import REPO
from test_ifc_import import mesh_volume, objects

HERE = Path(__file__).parent
MAKE = HERE / "ifc_make_solids.py"
VOLUMES = HERE / "ifc_volumes.py"


def uv_run(script, *args):
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    run = subprocess.run(
        ["uv", "run", "--no-project", "--with", "ifcopenshell", "python", "-I", str(script), *map(str, args)],
        capture_output=True, text=True, encoding="utf-8", timeout=600,
    )
    if run.returncode != 0 and "No solution found" in run.stderr + run.stdout:
        pytest.skip("IfcOpenShell is not available")
    assert run.returncode == 0, run.stderr
    return run.stdout


@pytest.fixture(scope="module")
def made(tmp_path_factory):
    out = tmp_path_factory.mktemp("ifc_solids")
    uv_run(MAKE, out)
    return out


def reference_volumes(path):
    return json.loads(uv_run(VOLUMES, path).splitlines()[-1])


def kadakas_volumes(cad, path):
    r = cad.result("import.file", {"path": str(path)})
    out = {}
    for o in objects(cad):
        out[o["data"]["ifc"]["name"]] = mesh_volume(o["shape"])
    return r, out


@pytest.mark.parametrize("name, cut_openings", [("wall_door_window.ifc", 2), ("slab_hole_pocket.ifc", 2)])
def test_openings_are_cut_like_ifcopenshell_does(cad, made, name, cut_openings):
    ref = reference_volumes(made / name)
    r, got = kadakas_volumes(cad, made / name)
    assert len(got) == len(ref) == 1, (got, ref)
    ((n, v),) = got.items()
    assert v == approx(ref[n]["volume"], rel=0.002)
    assert any(f"{cut_openings} openings (IfcRelVoidsElement) were cut" in note for note in r["report"]["notes"])
    assert r["report"]["entities_read"] == r["report"]["entities_imported"] + r["report"]["entities_skipped"]


def test_a_wall_with_an_opening_from_the_hand_written_fixture(cad):
    r = cad.result("import.file", {"path": str(REPO / "tests" / "fixtures" / "ifc" / "wall_with_openings.ifc")})
    assert r["imported"] == 1
    volume = mesh_volume(objects(cad)[0]["shape"])
    assert volume == approx(4000 * 200 * 2800 - 1000 * 200 * 2100 - 800 * 200 * 1200, rel=1e-9)
