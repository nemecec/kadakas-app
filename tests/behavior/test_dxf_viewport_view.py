"""Scenario: the twist and the perspective view of a viewport go through DXF (IOP-27).

A plan viewport can be twisted: the model turns about the center of the view. A 3D viewport can have a
perspective camera. The export writes both, the import reads both. The twist is checked against
the transformation that the ezdxf reader makes for a VIEWPORT (`get_transformation_matrix`).
"""

import math

import pytest
from pytest import approx

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
from ezdxf.math import Vec3  # noqa: E402

from helpers import rect_cmd  # noqa: E402


def paper_of(vp, p):
    """Where Kadakas draws the model point p: the center of the frame plus the turned offset."""
    c = [(vp["min"][i] + vp["max"][i]) / 2 for i in range(2)]
    d = [(p[i] - vp["center"][i]) / vp["scale"] for i in range(2)]
    t = math.radians(vp.get("twist", 0.0))
    return (c[0] + d[0] * math.cos(t) - d[1] * math.sin(t), c[1] + d[0] * math.sin(t) + d[1] * math.cos(t))


@pytest.fixture
def sheet(cad):
    cad.execute(rect_cmd(0, 0, 12000, 8000))
    cad.result("sheet.create", {"name": "Plan", "paper": "a3"})
    return cad


def test_a_twisted_plan_viewport_is_written_with_the_twist_that_ezdxf_applies(sheet, tmp_path):
    vp = sheet.result("viewport.create", {"sheet": "Plan", "scale": 100, "center": [6000, 4000], "twist": 30})
    assert vp["twist"] == 30
    out = tmp_path / "t.dxf"
    sheet.result("export.file", {"path": str(out), "format": "dxf", "options": {"version": "AC1027"}})
    doc = ezdxf.readfile(str(out))
    assert not doc.audit().errors
    layout = doc.layouts.get("Plan")
    viewports = [v for v in layout.viewports() if v.dxf.id != 1]
    assert len(viewports) == 1
    dv = viewports[0]
    assert dv.dxf.view_twist_angle == approx(30)
    m = dv.get_transformation_matrix()
    for p in [(6000, 4000), (0, 0), (12000, 8000), (3000, 7000)]:
        want = paper_of(vp, p)
        got = m.transform(Vec3(p[0], p[1], 0))
        assert (got.x, got.y) == approx(want, abs=1e-6), (p, got, want)


def test_a_negative_twist_is_written_from_0_to_360_and_read_back(sheet, tmp_path):
    vp = sheet.result("viewport.create", {"sheet": "Plan", "scale": 50, "center": [2000, 1000], "twist": -45})
    out = tmp_path / "t.dxf"
    sheet.result("export.file", {"path": str(out), "format": "dxf"})
    doc = ezdxf.readfile(str(out))
    dv = [v for v in doc.layouts.get("Plan").viewports() if v.dxf.id != 1][0]
    assert dv.dxf.view_twist_angle == approx(315)
    for p in [(2000, 1000), (5000, -300)]:
        got = dv.get_transformation_matrix().transform(Vec3(p[0], p[1], 0))
        assert (got.x, got.y) == approx(paper_of(vp, p), abs=1e-6)


def test_a_viewport_with_a_twist_and_a_view_center_is_read_where_ezdxf_puts_it(cad, tmp_path):
    doc = ezdxf.new("R2018")
    doc.units = 4
    layout = doc.layouts.new("A1")
    layout.add_text("Title", dxfattribs={"insert": (10, 10), "height": 5})
    dv = layout.add_viewport(center=(210, 148), size=(300, 200), view_center_point=(12, 7), view_height=2000)
    dv.dxf.view_twist_angle = 25
    dv.dxf.view_target_point = (1000, 2000, 0)
    doc.modelspace().add_line((0, 0), (5000, 0))
    path = tmp_path / "v.dxf"
    doc.saveas(str(path))
    r = cad.result("import.file", {"path": str(path), "options": {"unit": "mm"}})["report"]
    assert r["entities_read"] == r["entities_imported"] + r.get("entities_skipped", 0)
    assert any("1 plan viewport has a twist" in n for n in r["notes"]), r["notes"]
    (vp,) = cad.result("viewport.list")["viewports"]
    vp = cad.result("viewport.get", {"id": vp["id"]})
    assert vp["twist"] == approx(25)
    # The point that the middle of the frame shows is the point that the ezdxf matrix maps there.
    m = dv.get_transformation_matrix()
    m.inverse()
    shown = m.transform(Vec3(210, 148, 0))
    assert vp["center"] == approx([shown.x, shown.y], abs=1e-6)
    assert vp["scale"] == approx(2000 / 200)


def test_a_perspective_view_is_stored_written_and_read_back(sheet, tmp_path):
    vp = sheet.result("viewport.create", {"sheet": "Plan", "scale": 100, "view": "right_isometric", "perspective": True})
    assert vp["perspective"] is True
    out = tmp_path / "p.dxf"
    sheet.result("export.file", {"path": str(out), "format": "dxf"})
    doc = ezdxf.readfile(str(out))
    dv = [v for v in doc.layouts.get("Plan").viewports() if v.dxf.id != 1][0]
    assert dv.dxf.status & 1
    r = sheet.result("import.file", {"path": str(out)})["report"]
    assert any("3D viewport has a perspective view" in n for n in r["notes"]), r["notes"]
    rows = sheet.result("viewport.list")["viewports"]
    assert any(sheet.result("viewport.get", {"id": row["id"]}).get("perspective") for row in rows[1:])


def test_the_viewport_can_be_changed_and_a_twist_moves_the_fit(sheet):
    vp = sheet.result("viewport.create", {"sheet": "Plan", "scale": "fit"})
    flat = vp["scale"]
    turned = sheet.result("viewport.update", {"id": vp["id"], "twist": 90, "fit": True})
    assert turned["twist"] == 90
    # The model is 12 m by 8 m. Turned by a quarter, it is 8 m wide and 12 m high: a larger scale number.
    assert turned["scale"] > flat
    assert turned["center"] == approx([6000, 4000], abs=1e-6)
    back = sheet.result("viewport.update", {"id": vp["id"], "twist": 0, "perspective": True})
    assert "twist" not in back or back["twist"] == 0
    assert back["perspective"] is True


def test_the_reply_warns_when_the_twist_or_the_perspective_is_not_applied(sheet):
    plan = sheet.result("viewport.create", {"sheet": "Plan", "scale": 100, "perspective": True})
    assert any("plan or a section has no perspective" in w for w in plan["warnings"]), plan
    iso = sheet.result("viewport.create", {"sheet": "Plan", "scale": 100, "view": "right_isometric", "twist": 20})
    assert any("not turned" in w for w in iso["warnings"]), iso
    # A raster picture is not drawn in a twisted plan view: the reply says so.
    sheet.execute({"op": "add_object", "shape": {"kind": "underlay", "name": "map", "min": [0, 0], "max": [1000, 1000], "width": 1, "height": 1, "png": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==", "opacity": 1.0, "source": ""}})
    turned = sheet.result("viewport.update", {"id": plan["id"], "twist": 30})
    assert any("raster picture" in w for w in turned.get("warnings", [])), turned
    flat = sheet.result("viewport.update", {"id": plan["id"], "twist": 0, "perspective": False})
    assert "warnings" not in flat
