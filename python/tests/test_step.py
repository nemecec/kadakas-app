"""STEP import through the generated Python client (IOP-11): the file becomes a mesh, the mesh
keeps its exact surfaces, and the triangles can be cut again finer."""

from __future__ import annotations

import math
from pathlib import Path

import pytest

from kadakas import KadakasError

DATA = Path(__file__).resolve().parents[2] / "tests" / "data" / "step"


def test_the_client_knows_the_step_calls(doc):
    assert "STEP" in doc.api.import_file.__doc__
    assert "exact data" in doc.api.mesh3d_exact_info.__doc__
    assert "again" in doc.api.mesh3d_retessellate.__doc__


def test_import_a_cylinder_read_its_radius_and_cut_it_finer(doc):
    r = doc.api.import_file(path=str(DATA / "cylinder.step"), options={"tolerance": 0.5})
    mesh = r["report"]["objects"][0]
    assert mesh["kind"] == "mesh3d" and r["report"]["format"] == "step"
    info = doc.api.mesh3d_exact_info(id=mesh["id"])
    cylinder = next(f for f in info["list"] if f["kind"] == "cylinder")
    assert cylinder["radius"] == 10.0
    want = math.pi * 100 * 25
    coarse = doc.api.mesh3d_retessellate(id=mesh["id"], tolerance=0.5)
    fine = doc.api.mesh3d_retessellate(id=mesh["id"], tolerance=0.01)
    assert abs(fine["volume_mm3"] - want) < abs(coarse["volume_mm3"] - want)
    assert fine["triangles"]["after"] > coarse["triangles"]["after"]


def test_refusals_have_error_ids(doc, tmp_path):
    r = doc.api.import_file(path=str(DATA / "tessellated.step"))
    mesh = r["report"]["objects"][0]["id"]
    with pytest.raises(KadakasError) as e:
        doc.api.mesh3d_exact_info(id=mesh)
    assert e.value.data.get("error_id") == "no_exact_data", e.value.data
    off = doc.api.import_file(path=str(DATA / "box.step"), options={"keep_exact": False})
    with pytest.raises(KadakasError) as e:
        doc.api.mesh3d_retessellate(id=off["report"]["objects"][0]["id"], tolerance=0.1)
    assert e.value.data.get("error_id") == "no_exact_data", e.value.data
    bad = tmp_path / "bad.step"
    bad.write_text("not a step file")
    with pytest.raises(KadakasError) as e:
        doc.api.import_file(path=str(bad))
    assert e.value.data.get("error_id") == "mesh_malformed", e.value.data
