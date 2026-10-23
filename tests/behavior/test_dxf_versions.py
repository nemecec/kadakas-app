"""Scenario: the DXF export writes the release that a CAD program, a laser cutter or an old tool
reads (IOP-15), and the import reads every release. Checked with ezdxf: it must open each file
and its audit must find no error."""

import pytest

from all_kinds import build_all_kinds

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")

R12_ENTITIES = {"LINE", "CIRCLE", "ARC", "POINT", "TEXT", "POLYLINE"}


def export(cad, tmp_path, version, name=None):
    path = tmp_path / (name or f"{version}.dxf")
    r = cad.result("export.file", {"format": "dxf", "path": str(path), "options": {"version": version}})
    doc = ezdxf.readfile(str(path))
    auditor = doc.audit()
    assert not auditor.errors, [e.message for e in auditor.errors]
    return doc, r["report"]


@pytest.mark.parametrize("version", ["AC1015", "AC1018", "AC1027", "AC1032"])
def test_the_full_export_opens_in_ezdxf_in_each_release(cad, tmp_path, version):
    build_all_kinds(cad)
    doc, report = export(cad, tmp_path, version)
    # ezdxf upgrades nothing: the header names the release that was asked for.
    assert doc.dxfversion == version
    assert report["release"].startswith(version)
    assert report["objects"]["massing"] == 3
    assert not report["skipped"], report["skipped"]
    assert len(list(doc.modelspace())) > 20


def test_release_2000_has_no_true_color(cad, tmp_path):
    cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}, "attrs": {"pen": [10, 200, 30, 255]}})
    doc, report = export(cad, tmp_path, "2000")
    (line,) = doc.modelspace().query("LINE")
    assert not line.dxf.hasattr("true_color")
    assert line.dxf.color > 0
    assert any("true colors" in n for n in report["notes"])
    doc, _ = export(cad, tmp_path, "AC1018")
    (line,) = doc.modelspace().query("LINE")
    assert line.dxf.true_color == 0x0AC81E


def test_release_12_holds_plain_entities_only_and_counts_the_rest(cad, tmp_path):
    made = build_all_kinds(cad)
    doc, report = export(cad, tmp_path, "AC1009", "r12.dxf")
    assert doc.dxfversion == "AC1009"
    types = {e.dxftype() for e in doc.modelspace()}
    assert types <= R12_ENTITIES, types
    assert {"LINE", "CIRCLE", "ARC", "TEXT"} <= types
    # Everything that release 12 cannot hold is in the report, by kind, with a count.
    skipped = {s["entity"]: s["count"] for s in report["skipped"]}
    assert skipped.get("massing") == 3
    assert "irrigation_valve" in skipped or any(k.startswith("irrigation") for k in skipped), skipped
    assert all("release 12" in s["reason"] for s in report["skipped"])
    assert any("no unit" in n for n in report["notes"])
    assert made["massing"]
    # The file is in the code page ANSI_1252, so the old tools read it.
    assert doc.header["$DWGCODEPAGE"] == "ANSI_1252"


def test_release_12_round_trip_keeps_geometry(cad, tmp_path):
    cad.execute({"op": "add_object", "shape": {"kind": "polyline", "points": [[0, 0], {"p": [500, 200], "kind": "point_on_arc"}, [1000, 0]], "closed": False}})
    cad.execute({"op": "add_object", "shape": {"kind": "text", "at": [0, 500], "text": "Käed", "height": 100}})
    doc, _ = export(cad, tmp_path, "R12", "round.dxf")
    (poly,) = doc.modelspace().query("POLYLINE")
    assert any(abs(v.dxf.bulge) > 0.01 for v in poly.vertices)
    (text,) = doc.modelspace().query("TEXT")
    assert text.dxf.text == "Käed"
    r = cad.result("import.file", {"path": str(tmp_path / "round.dxf")})
    assert r["report"]["release"].startswith("AC1009")
    assert r["imported"] == 2


def test_a_bad_release_is_refused(cad, tmp_path):
    err = cad.error("export.file", {"format": "dxf", "path": str(tmp_path / "x.dxf"), "options": {"version": "AC9999"}})
    assert "version" in err["message"] or "AC9999" in err["message"]
