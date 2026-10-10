"""Scenario: the same garden is exported as IFC2x3 and IFC4x3. IfcOpenShell validates both files."""

from pytest import approx

from test_ifc_export import garden, read_ifc


def test_ifc2x3_export_validates_and_keeps_the_garden(cad, tmp_path):
    garden(cad)
    out = tmp_path / "garden2x3.ifc"
    r = cad.result("export.file", {"path": str(out), "format": "ifc", "options": {"version": "IFC2X3"}})
    assert r["exported"] == 7
    text = out.read_text()
    assert "FILE_SCHEMA(('IFC2X3'));" in text and "IFCOWNERHISTORY" in text
    assert "IFCGEOGRAPHICELEMENT" not in text and "IFCTRIANGULATEDFACESET" not in text
    assert "IFCMAPCONVERSION" not in text
    facts = read_ifc(out)
    assert facts["schema"] == "IFC2X3"
    assert facts["validation"] == [], facts["validation"]
    counts = facts["counts"]
    assert counts["IfcSlab"] == 2 and counts["IfcBuildingElementProxy"] == 5 and counts["IfcSite"] == 1
    assert counts["IfcFaceBasedSurfaceModel"] >= 2
    for p in facts["products"]:
        assert p["triangles"] > 0, p["name"]
        assert p["container"] == "IfcSite"
    assert facts["unit_scale"] == approx(0.001)
    # The georeference is in property sets of the site.
    assert {"ePSet_MapConversion", "ePSet_ProjectedCRS"} <= set(facts["psets"])
    terrain = [p for p in facts["products"] if p["object_type"] == "TERRAIN"]
    assert len(terrain) == 1
    assert facts["ref_latitude"][0] == 60 and len(facts["ref_latitude"]) == 3
    slab = next(p for p in facts["products"] if p["type"] == "IfcSlab")
    assert slab["material"] == "IfcMaterialLayerSetUsage"
    assert slab["psets"]["Qto_SlabBaseQuantities"]["GrossArea"] == approx(6.0)


def test_ifc4x3_export_validates_and_has_vegetation_and_terrain(cad, tmp_path):
    garden(cad)
    out = tmp_path / "garden4x3.ifc"
    cad.result("export.file", {"path": str(out), "format": "ifc", "options": {"version": "IFC4X3", "include_2d": True}})
    assert "FILE_SCHEMA(('IFC4X3_ADD2'));" in out.read_text()
    facts = read_ifc(out)
    assert facts["schema"] in ("IFC4X3", "IFC4X3_ADD2")
    assert facts["validation"] == [], facts["validation"]
    geo = [p for p in facts["products"] if p["type"] == "IfcGeographicElement"]
    assert sorted(p["predefined"] for p in geo) == ["TERRAIN"] + ["VEGETATION"] * 4
    assert all(p["object_type"] is None for p in geo if p["predefined"] == "VEGETATION")
    assert facts["crs"] == "EPSG:3067"
    assert facts["map_conversion"]["Eastings"] == 385000 and facts["map_conversion"]["Northings"] == 6672000
    assert facts["counts"]["IfcAnnotation"] == 2
    assert facts["ref_latitude"][0] == 60


def test_ifc4_stays_the_default_and_other_versions_are_refused(cad, tmp_path):
    garden(cad)
    out = tmp_path / "default.ifc"
    cad.result("export.file", {"path": str(out), "format": "ifc"})
    assert "FILE_SCHEMA(('IFC4'));" in out.read_text()
    err = cad.error("export.file", {"path": str(tmp_path / "x.ifc"), "format": "ifc", "options": {"version": "IFC5"}})
    assert err["code"] == -32602
