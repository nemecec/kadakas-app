"""Import and export of meshes through the generated Python client (MOD-12): OBJ and STL in,
`mesh_to_surface`, the soften option and its count, STL out, and the refusals."""

from __future__ import annotations

import pytest

from kadakas import KadakasError

CUBE_OBJ = (
    "o Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n"
    "f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n"
)


def test_the_client_knows_the_new_calls(doc):
    assert "Turn a mesh object" in doc.api.mesh_to_surface.__doc__
    assert "import.file" in doc.api.import_file.__doc__


def test_import_a_mesh_convert_it_and_soften(doc, tmp_path):
    obj = tmp_path / "cube.obj"
    obj.write_text(CUBE_OBJ, encoding="utf-8")
    r = doc.api.import_file(path=str(obj), options={"unit": "mm", "up_axis": "z"})
    assert r["imported"] == 1 and r["softened_edges"] == 0
    mesh = r["report"]["objects"][0]
    assert mesh["kind"] == "mesh3d" and mesh["triangles"] == 12
    dry = doc.api.mesh_to_surface(id=mesh["id"], soften_below_angle=100, dry_run=True)
    assert dry["softened_edges"] == 12
    done = doc.api.mesh_to_surface(id=mesh["id"], soften_below_angle=100)
    assert done["kind"] == "surface_model" and done["report"]["faces"] == 6
    summary = doc.api.mesh_summary(id=mesh["id"])["models"][0]
    assert summary["soft_edges"] == 12
    with pytest.raises(KadakasError) as e:
        doc.api.mesh_to_surface(id=mesh["id"])
    assert e.value.data.get("error_id") == "not_a_mesh", e.value.data


def test_export_stl_and_refuse_a_broken_file(doc, tmp_path):
    obj = tmp_path / "cube.obj"
    obj.write_text(CUBE_OBJ, encoding="utf-8")
    doc.api.import_file(path=str(obj), options={"to_surface": True})
    out = tmp_path / "cube.stl"
    r = doc.api.export_file(path=str(out), format="stl", options={"encoding": "ascii"})
    assert r["report"]["triangles"] == 12 and r["report"]["open_models"] == 0
    assert out.read_text(encoding="utf-8").startswith("solid cube")
    broken = tmp_path / "cut.stl"
    broken.write_bytes(out.read_bytes()[:50])
    with pytest.raises(KadakasError) as e:
        doc.api.import_file(path=str(broken))
    assert e.value.data.get("reason") in ("mesh_malformed", "mesh_truncated"), e.value.data
