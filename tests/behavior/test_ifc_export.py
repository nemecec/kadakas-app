"""Scenario: a landscape architect exports a garden as IFC4. IfcOpenShell (through `uv`) reads the file:
it must validate without errors, have the right spatial tree, property sets and georeference."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from pytest import approx

CHECK = Path(__file__).with_name("ifc_check.py")
SQUARE = [[0, 0], [3000, 0], [3000, 2000], [0, 2000]]


def read_ifc(path):
    """Facts about an IFC file from IfcOpenShell. Skips the test if `uv` or IfcOpenShell is not available."""
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    run = subprocess.run(
        ["uv", "run", "--no-project", "--with", "ifcopenshell", "python", "-I", str(CHECK), str(path)],
        capture_output=True, text=True, timeout=600,
    )
    if run.returncode != 0 and "No solution found" in run.stderr + run.stdout:
        pytest.skip("IfcOpenShell is not available")
    assert run.returncode == 0, run.stderr
    return json.loads(run.stdout.splitlines()[-1])


def garden(cad):
    cad.execute({"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000, 6672000], "rotation": 0}})
    cad.result("project.set", {"name": "Pähkinäpuisto", "client": "Client Oy"})
    ids = []
    for i in range(9):
        for j in range(9):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, 200 + i * 50]}})
    cad.result("site.create", {"source_ids": ids})
    cad.result("hardscape_style.create", {"name": "Granite setts", "material": "Granite", "thickness": 60,
                                          "components": [{"name": "Bedding", "material": "Sand", "thickness": 40}]})
    cad.result("hardscape.add", {"boundary": SQUARE, "style": "Granite setts"})
    cad.result("hardscape.add", {"boundary": [[4000, 0], [6000, 0], [6000, 2000], [4000, 2000]], "style": "Granite setts", "drape": True})
    cad.result("plant_style.create", {"name": "Field maple", "botanical_name": "Acer campestre", "id_code": "AC", "category": "tree",
                                      "spread": 5000, "height": 9000, "price": 100})
    cad.result("plants.place", {"mode": "poly", "style": "Field maple", "points": [[1000, 4000], [3000, 4000]]})
    cad.result("landscape_area.add", {"boundary": [[4000, 3000], [7000, 3000], [7000, 5000], [4000, 5000]], "name": "Meadow"})
    cad.execute({"op": "add_object", "shape": {"kind": "existing_tree", "at": [7000, 7000], "species": "Betula pendula", "dbh": 400, "spread": 6000, "height": 12000}})
    cad.execute({"op": "add_object", "shape": {"kind": "text", "at": [0, 0], "text": "Note", "height": 250}})
    cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [100, 100]}})


def test_export_is_valid_ifc4_with_tree_data_and_georeference(cad, tmp_path):
    garden(cad)
    out = tmp_path / "garden.ifc"
    r = cad.result("export.file", {"path": str(out), "format": "ifc"})
    assert r["exported"] == 7
    skipped = {s["entity"] for s in r["report"]["skipped"]}
    assert skipped == {"text", "line"}
    assert len(r["project_guid"]) == 22
    assert out.read_text().startswith("ISO-10303-21;")
    assert "FILE_SCHEMA(('IFC4'))" in out.read_text()

    facts = read_ifc(out)
    assert facts["schema"] == "IFC4"
    assert facts["validation"] == [], "IfcOpenShell finds no schema errors"
    assert facts["project"] == "Pähkinäpuisto"
    assert facts["unit_scale"] == approx(0.001)
    counts = facts["counts"]
    assert counts["IfcSlab"] == 2 and counts["IfcGeographicElement"] == 5 and counts["IfcSite"] == 1
    assert "IfcBuilding" not in counts, "a landscape project has no building"

    # The spatial tree: project, site, and every element in the site.
    site = facts["tree"]["children"][0]
    assert site["type"] == "IfcSite"
    assert len(site["children"]) == 7
    # Every geometry has triangles, and the enumerations are valid IFC4 values.
    by_type = {}
    for p in facts["products"]:
        assert p["triangles"] > 0, p["name"]
        assert p["container"] == "IfcSite"
        by_type.setdefault(p["type"], []).append(p)
    assert {p["predefined"] for p in by_type["IfcSlab"]} == {"BASESLAB"}
    assert sorted(p["predefined"] for p in by_type["IfcGeographicElement"]) == ["TERRAIN"] + ["USERDEFINED"] * 4
    assert {p["object_type"] for p in by_type["IfcGeographicElement"] if p["predefined"] == "USERDEFINED"} == {"VEGETATION"}

    slab = by_type["IfcSlab"][0]
    assert slab["material"] == "IfcMaterialLayerSetUsage"
    assert slab["psets"]["Pset_SlabCommon"]["IsExternal"] is True
    assert slab["psets"]["Qto_SlabBaseQuantities"]["Depth"] == approx(100)
    assert slab["psets"]["Qto_SlabBaseQuantities"]["GrossArea"] == approx(6.0)
    maple = next(p for p in by_type["IfcGeographicElement"] if p["name"] == "Field maple")
    assert maple["psets"]["Cadektor_Planting"]["BotanicalName"] == "Acer campestre"
    meadow = next(p for p in by_type["IfcGeographicElement"] if p["name"] == "Meadow")
    assert meadow["psets"]["Cadektor_AreaQuantities"]["GrossArea"] == approx(6.0)

    # The georeference: map conversion, CRS name, and latitude and longitude of the site.
    assert facts["crs"] == "EPSG:3067"
    assert facts["map_conversion"]["Eastings"] == 385000 and facts["map_conversion"]["Northings"] == 6672000
    assert facts["map_conversion"]["Scale"] == approx(0.001)
    assert facts["ref_latitude"][0] == 60 and facts["ref_longitude"][0] == 24


def test_export_is_the_same_every_time_and_ids_follow_objects(cad, tmp_path):
    garden(cad)
    out = tmp_path / "same.ifc"
    cad.result("export.file", {"path": str(out), "format": "ifc"})
    first = out.read_bytes()
    cad.result("export.file", {"path": str(out), "format": "ifc", "overwrite": True})
    assert out.read_bytes() == first
    # The GlobalIds survive saving and loading the document.
    doc = cad.result("document.get")
    cad.result("document.load", {"document": doc})
    cad.result("export.file", {"path": str(out), "format": "ifc", "overwrite": True})
    assert out.read_bytes() == first


def test_options_storeys_2d_and_layers(cad, tmp_path):
    garden(cad)
    out = tmp_path / "options.ifc"
    cad.result("export.file", {"path": str(out), "options": {"layers_as_storeys": True, "include_2d": True, "classes_as_layers": True}})
    facts = read_ifc(out)
    assert facts["validation"] == []
    assert facts["counts"]["IfcBuilding"] == 1 and facts["counts"]["IfcBuildingStorey"] == 1
    assert facts["counts"]["IfcAnnotation"] == 2
    cad.result("export.file", {"path": str(out), "options": {"classes_as_layers": False}, "overwrite": True})
    assert "IFCPRESENTATIONLAYERASSIGNMENT" not in out.read_text()


def test_bad_options_and_path_policy(cad, tmp_path):
    garden(cad)
    out = tmp_path / "x.ifc"
    assert cad.error("export.file", {"path": str(out), "format": "ifc", "options": {"version": "IFC2X3"}})["code"] == -32602
    assert cad.error("export.file", {"path": str(out), "format": "ifc", "options": {"colour": 1}})["code"] == -32602
    assert not out.exists()
    # The extension must match the format, and an existing file needs overwrite.
    assert cad.error("export.file", {"path": str(tmp_path / "x.txt"), "format": "ifc"})["code"] == 1
    cad.result("export.file", {"path": str(out)})
    assert cad.error("export.file", {"path": str(out), "format": "ifc"})["code"] == 1
    cad.result("export.file", {"path": str(out), "format": "ifc", "overwrite": True})


def test_describe_lists_the_ifc_export(cad):
    d = cad.result("describe")
    assert "ifc" in " ".join(d["io"]["export_formats"])
    assert "layers_as_storeys" in d["io"]["ifc"]["options"]
