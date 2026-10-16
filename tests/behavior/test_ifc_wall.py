"""Scenario: a landscape wall (a polyline with wall settings) is an IfcWall in the IFC export, with
its solid, the layers of its style as materials and quantities. IfcOpenShell reads the files."""

import json
import subprocess
import sys

import pytest
from pytest import approx

ifcopenshell = pytest.importorskip("ifcopenshell", reason="run with: uv run --with pytest --with ifcopenshell pytest tests/behavior")
import ifcopenshell.geom  # noqa: E402
import ifcopenshell.util.element as element  # noqa: E402


def build(cad):
    """A sloping site (100 mm per meter) with a plain wall, a styled wall with coping, and a retaining wall."""
    ids = []
    for i in range(11):
        for j in range(5):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}})
    cad.result("site.create", {"source_ids": ids})
    cad.result("wall_style.create", {"name": "Block wall", "material": "Concrete",
                                     "components": [{"name": "Block", "material": "Concrete", "thickness": 200},
                                                    {"name": "Render", "material": "Lime", "thickness": 20}],
                                     "coping": {"height": 50, "overhang": 20, "material": "Stone"}})
    styled = cad.result("wall.add", {"path": [[1000, 2000], [6000, 2000]], "top": {"mode": "terrain", "offset": 600}, "style": "Block wall"})["id"]
    plain = cad.result("wall.add", {"path": [[1000, 1000], [4000, 1000]], "top": {"mode": "fixed", "elevation": 1500}})["id"]
    retaining = cad.result("wall.add", {"path": [[7000, 500], [7000, 3500]], "top": {"mode": "fixed", "elevation": 1200}, "site_modifier": True})["id"]
    return styled, plain, retaining


def export(cad, tmp_path, version=None):
    path = tmp_path / f"walls_{version or 'ifc4'}.ifc"
    options = {"version": version} if version else {}
    r = cad.result("export.file", {"path": str(path), "format": "ifc", "options": options})
    return ifcopenshell.open(str(path)), r


def validation_statements(path):
    """The messages of the IfcOpenShell validation, from a separate Python process: importing
    ifcopenshell.validate puts a module named `codegen` in sys.modules, which would shadow the
    `codegen` of python/tests/test_drift.py."""
    script = (
        "import json, sys, ifcopenshell, ifcopenshell.validate as v\n"
        "m = ifcopenshell.open(sys.argv[1]); log = v.json_logger(); v.validate(m, log)\n"
        "print(json.dumps(log.statements))\n"
    )
    run = subprocess.run([sys.executable, "-I", "-c", script, str(path)], capture_output=True, text=True, check=True)
    return json.loads(run.stdout.splitlines()[-1])


def z_range(model, wall):
    settings = ifcopenshell.geom.settings()
    shape = ifcopenshell.geom.create_shape(settings, wall)
    zs = shape.geometry.verts[2::3]
    return min(zs) * 1000, max(zs) * 1000  # the geometry is in meters


@pytest.mark.parametrize("version", [None, "IFC4X3", "IFC2X3"])
def test_walls_are_ifc_walls_with_solid_materials_and_quantities(cad, tmp_path, version):
    build(cad)
    model, r = export(cad, tmp_path, version)
    assert not [s for s in r["report"]["skipped"] if s["entity"] == "polyline"], r["report"]["skipped"]
    assert r["report"]["objects"]["polyline"] == 3
    statements = validation_statements(tmp_path / f"walls_{version or 'ifc4'}.ifc")
    assert statements == [], statements[:3]
    walls = model.by_type("IfcWall")
    assert len(walls) == 3
    by_name = {}
    for w in walls:
        by_name.setdefault(w.Name, []).append(w)
    (styled,) = by_name["Block wall"]
    assert len(by_name["Landscape wall"]) == 2

    # The solid: the wall stands on the ground (100 mm at x = 1 m, 600 mm at x = 6 m) and its top is
    # 600 above it, with a 50 mm coping on top.
    lo, hi = z_range(model, styled)
    assert lo == approx(100, abs=1) and hi == approx(600 + 600 + 50, abs=1)

    # Materials: the components of the style, as layers.
    mat = element.get_material(styled)
    layers = mat.ForLayerSet.MaterialLayers if mat.is_a("IfcMaterialLayerSetUsage") else mat.MaterialLayers
    assert [(layer.Material.Name, layer.LayerThickness) for layer in layers] == [("Concrete", 200), ("Lime", 20)]

    # Quantities, in mm and m2 / m3 (the file's units).
    psets = element.get_psets(styled, qtos_only=True)
    q = psets["Qto_WallBaseQuantities"]
    assert q["Length"] == approx(5000, rel=1e-3)
    assert q["Width"] == approx(220)
    assert q["Height"] == approx(600, abs=1)
    assert q["NetSideArea"] == approx(5.0 * 0.6, rel=1e-3)
    assert q["NetVolume"] == approx(5.0 * 0.6 * 0.22, rel=1e-3)
    common = element.get_psets(styled)["Pset_WallCommon"]
    assert common["Reference"] == "Block wall" and common["IsExternal"] is True

    # Predefined type and object type: a retaining wall says so.
    retaining = [w for w in by_name["Landscape wall"] if element.get_psets(w)["Pset_WallCommon"]["LoadBearing"]]
    assert len(retaining) == 1
    plain = [w for w in by_name["Landscape wall"] if w not in retaining][0]
    if version == "IFC2X3":
        assert retaining[0].ObjectType == "RETAININGWALL" and plain.ObjectType == "LANDSCAPEWALL"
    elif version == "IFC4X3":
        assert retaining[0].PredefinedType == "RETAININGWALL" and plain.PredefinedType == "SOLIDWALL"
        assert plain.ObjectType == "LANDSCAPEWALL"
    else:
        assert retaining[0].PredefinedType == "USERDEFINED" and retaining[0].ObjectType == "RETAININGWALL"
        assert plain.PredefinedType == "SOLIDWALL" and plain.ObjectType == "LANDSCAPEWALL"
    # The plain wall has a fixed top at 1500 and stands on the sloping ground.
    lo, hi = z_range(model, plain)
    assert hi == approx(1500, abs=1) and lo == approx(100, abs=1)
    # A wall is inside the spatial structure.
    assert all(element.get_container(w) is not None for w in walls)
