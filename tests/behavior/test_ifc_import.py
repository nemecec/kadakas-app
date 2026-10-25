"""Scenario: a landscape architect gets an IFC file from an architect or an engineer and imports it.
Files from IfcOpenShell (through `uv`) stand for files from other programs. The hand-written files in
tests/fixtures/ifc cover each entity type. Our own export must come back in."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from pytest import approx

from helpers import REPO
from test_ifc_export import read_ifc

MAKE = Path(__file__).with_name("ifc_make.py")
FIXTURES = REPO / "tests" / "fixtures" / "ifc"
SQUARE = [[0, 0], [3000, 0], [3000, 2000], [0, 2000]]


@pytest.fixture(scope="module")
def generated(tmp_path_factory):
    """Files made by IfcOpenShell. Skips the tests if `uv` or IfcOpenShell is not available."""
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    out = tmp_path_factory.mktemp("ifcopenshell")
    run = subprocess.run(
        ["uv", "run", "--no-project", "--with", "ifcopenshell", "python", "-I", str(MAKE), str(out)],
        capture_output=True, text=True, encoding="utf-8", timeout=600,
    )
    if run.returncode != 0 and "No solution found" in run.stderr + run.stdout:
        pytest.skip("IfcOpenShell is not available")
    assert run.returncode == 0, run.stderr
    return out


def objects(cad):
    return list(cad.doc()["objects"].values()) if isinstance(cad.doc()["objects"], dict) else cad.doc()["objects"]


def mesh_volume(shape):
    v = shape["vertices"]
    total = 0.0
    for a, b, c in shape["triangles"]:
        pa, pb, pc = v[a], v[b], v[c]
        cross = (pb[1] * pc[2] - pb[2] * pc[1], pb[2] * pc[0] - pb[0] * pc[2], pb[0] * pc[1] - pb[1] * pc[0])
        total += pa[0] * cross[0] + pa[1] * cross[1] + pa[2] * cross[2]
    return abs(total) / 6.0


def test_wall_with_an_opening_is_a_mesh_and_the_opening_is_reported(cad, generated):
    r = cad.result("import.file", {"path": str(generated / "wall_opening.ifc")})
    assert r["imported"] == 1
    shape = objects(cad)[0]["shape"]
    assert shape["kind"] == "mesh3d"
    assert mesh_volume(shape) == approx(5000 * 250 * 2400, rel=1e-6)
    data = objects(cad)[0]["data"]
    assert data["ifc_class"] == "IfcWall" and data["ifc"]["name"] == "Garden wall"
    skipped = {(s["entity"], s["reason"]) for s in r["report"]["skipped"]}
    assert any(e == "IfcOpeningElement" for e, _ in skipped)
    assert any("openings" in n and "not cut" in n for n in r["report"]["notes"])
    # The property set is a record.
    record = data["Pset_WallCommon"]
    assert record["IsExternal"] is True and record["FireRating"] == "EI30"


def test_slab_with_voids_keeps_its_holes(cad, generated):
    cad.result("import.file", {"path": str(generated / "slab_voids.ifc")})
    shape = objects(cad)[0]["shape"]
    area = 8000 * 5000 - 1500 * 1500 - 2000 * 2000
    assert mesh_volume(shape) == approx(area * 150, rel=1e-6)
    zs = [v[2] for v in shape["vertices"]]
    assert min(zs) == approx(300) and max(zs) == approx(450)


def test_mapped_bollards_become_one_symbol_and_four_instances(cad, generated):
    r = cad.result("import.file", {"path": str(generated / "mapped_bollards.ifc")})
    assert r["report"]["objects"] == {"symbol": 4}
    doc = cad.doc()
    symbols = [o for o in objects(cad) if o["shape"]["kind"] == "symbol"]
    assert len({o["shape"]["symbol"] for o in symbols}) == 1
    assert sorted(o["shape"]["at"][0] for o in symbols) == [1000, 3000, 5000, 7000]
    assert len([r for r in doc["resources"] if r["name"].startswith("IFC mapped shape")]) == 1
    # Without instancing the bollards are meshes.
    cad.result("undo")
    r = cad.result("import.file", {"path": str(generated / "mapped_bollards.ifc"), "options": {"instances": False}})
    assert r["report"]["objects"] == {"mesh3d": 4}


def test_terrain_and_tree(cad, generated):
    r = cad.result("import.file", {"path": str(generated / "terrain_tree.ifc")})
    assert r["report"]["objects"] == {"site_model": 1, "plant": 1}
    plant = next(o for o in objects(cad) if o["shape"]["kind"] == "plant")["shape"]
    assert plant["at"] == [approx(4000, abs=1), approx(3000, abs=1)]
    assert plant["height"] == approx(5000) and plant["spread"] == approx(4000, abs=15)
    site = next(o for o in objects(cad) if o["shape"]["kind"] == "site_model")["shape"]
    assert len(site["points"]) == 4 and max(p[2] for p in site["points"]) == 1000


def test_ifc2x3_file(cad, generated):
    r = cad.result("import.file", {"path": str(generated / "wall_ifc2x3.ifc")})
    assert r["imported"] == 1
    assert objects(cad)[0]["data"]["ifc"]["schema"] == "IFC2X3"


def test_import_is_one_undo_step(cad, generated):
    cad.result("import.file", {"path": str(generated / "wall_opening.ifc")})
    assert len(objects(cad)) == 1
    cad.result("undo")
    assert objects(cad) == []
    cad.result("redo")
    assert len(objects(cad)) == 1


@pytest.mark.parametrize("name", sorted(p.name for p in FIXTURES.glob("*.ifc")))
def test_every_fixture_imports_and_survives_save(cad, make_serve, name, tmp_path):
    r = cad.result("import.file", {"path": str(FIXTURES / name)})
    assert r["report"]["format"] == "ifc"
    # The document stays valid: it saves and loads again.
    saved = tmp_path / "out.kad"
    expected = cad.doc()
    cad.result("file.save", {"path": str(saved)})
    other = make_serve()
    other.result("file.open", {"path": str(saved)})
    assert other.doc() == expected


def test_georeference_storeys_and_classes_from_a_fixture(cad):
    r = cad.result("import.file", {"path": str(FIXTURES / "georef_storeys.ifc")})
    assert r["georeference"]["crs"] == "EPSG:3067"
    assert r["georeference"]["origin"] == [385000, 6672000]
    doc = cad.doc()
    layers = {l["name"]: l for l in doc["layers"]}
    assert layers["First floor"]["elevation"] == 3200
    assert {c["name"] for c in doc["classes"]} >= {"A-FLOR", "A-FLOR-UPPER"}


def test_our_own_export_comes_back(cad, tmp_path, make_serve):
    cad.result("hardscape_style.create", {"name": "Setts", "material": "Granite", "thickness": 60})
    cad.result("hardscape.add", {"boundary": SQUARE, "style": "Setts"})
    cad.result("plant_style.create", {"name": "Maple", "botanical_name": "Acer", "id_code": "AC", "category": "tree", "spread": 5000, "height": 9000})
    cad.result("plants.place", {"mode": "poly", "style": "Maple", "points": [[1000, 4000]]})
    cad.result("landscape_area.add", {"boundary": [[4000, 3000], [7000, 3000], [7000, 5000], [4000, 5000]], "name": "Meadow"})
    cad.execute({"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000, 6672000], "rotation": 0}})
    out = tmp_path / "garden.ifc"
    cad.result("export.file", {"path": str(out), "format": "ifc"})

    other = make_serve()
    r = other.result("import.file", {"path": str(out)})
    kinds = sorted(o["shape"]["kind"] for o in objects(other))
    assert kinds == ["landscape_area", "mesh3d", "plant"], kinds
    assert r["georeference"]["crs"] == "EPSG:3067"
    slab = next(o for o in objects(other) if o["shape"]["kind"] == "mesh3d")
    assert slab["data"]["ifc_class"] == "IfcSlab"
    # A slab of the style is 60 mm thick (the components are inside this thickness).
    assert mesh_volume(slab["shape"]) == approx(3000 * 2000 * 60, rel=1e-3)
    area = next(o for o in objects(other) if o["shape"]["kind"] == "landscape_area")["shape"]
    assert area["name"] == "Meadow"


def test_errors_and_paths(cad, tmp_path):
    error = cad.error("import.file", {"path": str(tmp_path / "missing.ifc")})
    assert "cannot read" in error["message"]
    bad = tmp_path / "bad.ifc"
    bad.write_text("this is not a STEP file", encoding="utf-8")
    assert "not a STEP file" in cad.error("import.file", {"path": str(bad)})["message"]
    assert "unknown option" in cad.error("import.file", {"path": str(FIXTURES / "units_feet.ifc"), "options": {"nope": 1}})["message"]
    # The extension must match in a restricted session.
    wrong = tmp_path / "model.txt"
    wrong.write_text((FIXTURES / "units_feet.ifc").read_text(encoding="utf-8"))
    assert cad.error("import.file", {"path": str(wrong), "format": "ifc"}) is not None


def test_describe_lists_the_ifc_import(cad):
    d = cad.result("describe")
    ifc = d["io"]["ifc"]["import"]
    assert "layers_from_storeys" in ifc["options"] and "mapping" in ifc
    assert any("ifc" in f for f in d["io"]["import_formats"])


def test_imported_meshes_export_as_valid_ifc(cad, generated, tmp_path):
    """A file from IfcOpenShell comes in as meshes and goes out again. IfcOpenShell validates and reads the result."""
    cad.result("import.file", {"path": str(generated / "slab_voids.ifc")})
    cad.result("import.file", {"path": str(generated / "wall_opening.ifc")})
    out = tmp_path / "again.ifc"
    cad.result("export.file", {"path": str(out), "format": "ifc"})
    facts = read_ifc(out)
    assert facts["validation"] == []
    assert facts["counts"]["IfcSlab"] == 1
    assert facts["counts"]["IfcBuildingElementProxy"] == 1
    assert all(p["triangles"] > 0 for p in facts["products"])


def test_mapped_items_land_where_ifcopenshell_puts_them(cad, generated):
    """A map origin that is not at zero: the positions must be the same as the ones of IfcOpenShell."""
    expected = json.loads((generated / "mapped_origin.json").read_text(encoding="utf-8"))
    cad.result("import.file", {"path": str(generated / "mapped_origin.ifc"), "options": {"instances": False}})
    shape = objects(cad)[0]["shape"]
    for axis, (lo, hi) in enumerate(expected):
        values = [v[axis] for v in shape["vertices"]]
        assert min(values) == approx(lo, abs=1e-3) and max(values) == approx(hi, abs=1e-3), (axis, expected)
