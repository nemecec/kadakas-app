"""Scenario: IFC arrives as ifcZIP (Vectorworks, Revit and Allplan write it), and leaves as ifcZIP.
IFC XML and IFC JSON are refused with a clear message. Hostile zip files are stopped."""

import zipfile

from test_ifc_export import garden, read_ifc
from helpers import REPO

FIXTURE = REPO / "tests" / "fixtures" / "ifc" / "extruded_rect.ifc"


def zip_file(path, entries):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in entries:
            z.writestr(name, data)
    return path


def test_ifczip_import_gives_the_same_result_as_the_plain_file(cad, make_serve, tmp_path):
    plain = cad.result("import.file", {"path": str(FIXTURE)})
    packed = zip_file(tmp_path / "rect.ifczip", [("rect.ifc", FIXTURE.read_bytes())])
    other = make_serve()
    r = other.result("import.file", {"path": str(packed)})
    assert r["imported"] == plain["imported"] == 1
    assert r["report"]["notes"] == plain["report"]["notes"]
    # The zip is found by its content, whatever the name.
    odd = zip_file(tmp_path / "rect.bin.ifc", [("a.ifc", FIXTURE.read_bytes()), ("readme.txt", b"hi")])
    third = make_serve()
    r = third.result("import.file", {"path": str(odd), "format": "ifc"})
    assert r["imported"] == 1
    assert any("other entries in the zip" in n for n in r["report"]["notes"])


def test_ifczip_export_is_a_zip_with_one_ifc_that_ifcopenshell_reads(cad, tmp_path):
    garden(cad)
    out = tmp_path / "garden.ifczip"
    r = cad.result("export.file", {"path": str(out)})
    assert r["exported"] == 7
    with zipfile.ZipFile(out) as z:
        assert z.namelist() == ["garden.ifc"]
        assert z.testzip() is None
        z.extract("garden.ifc", tmp_path / "unpacked")
    facts = read_ifc(tmp_path / "unpacked" / "garden.ifc")
    assert facts["schema"] == "IFC4" and facts["validation"] == []
    assert facts["counts"]["IfcSlab"] == 2


def test_xml_json_and_hostile_zips_are_refused(cad, tmp_path):
    xml = tmp_path / "m.ifcxml"
    xml.write_text('<?xml version="1.0" encoding="UTF-8"?><ifcXML/>', encoding="utf-8")
    e = cad.error("import.file", {"path": str(xml), "format": "ifc"})
    assert "IFC XML is not supported" in e["message"]
    js = tmp_path / "m.ifcjson"
    js.write_text('{"type": "ifcJSON"}', encoding="utf-8")
    e = cad.error("import.file", {"path": str(js), "format": "ifc"})
    assert "IFC JSON is not supported" in e["message"]
    # A zip bomb: 90 MB of zeros is a few hundred KB of zip, and grows by more than 200 times.
    bomb = zip_file(tmp_path / "bomb.ifczip", [("b.ifc", bytes(90 * 1024 * 1024))])
    assert bomb.stat().st_size < 1_000_000
    e = cad.error("import.file", {"path": str(bomb)})
    assert "too big" in e["message"]
    # A path that leaves the folder.
    evil = zip_file(tmp_path / "evil.ifczip", [("../evil.ifc", FIXTURE.read_bytes())])
    e = cad.error("import.file", {"path": str(evil)})
    assert "unsafe name" in e["message"]
    # Two IFC files.
    two = zip_file(tmp_path / "two.ifczip", [("a.ifc", b"x"), ("b.ifc", b"y")])
    e = cad.error("import.file", {"path": str(two)})
    assert "more than one .ifc" in e["message"]
