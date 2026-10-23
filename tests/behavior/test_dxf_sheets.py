"""Scenario: the sheets of a design go to a CAD program as layouts, and a file with layouts comes
back as sheets (IOP-16). Checked with ezdxf: it must read each release, the audit must find no
error, and the layouts must hold the viewports, annotations and the title block."""

import pytest
from pytest import approx

from helpers import rect_cmd

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")


@pytest.fixture
def plan(cad):
    trees = cad.execute({"op": "add_class", "name": "Trees", "attrs": {"pen": [0, 0, 255, 255]}})[0]
    cad.execute(rect_cmd(0, 0, 12000, 8000, attrs={"pen": [120, 0, 0, 255]}))
    cad.execute({"op": "add_object", "class": trees, "shape": {"kind": "circle", "center": [15000, 4000], "radius": 2000}})
    cad.result("project.set", {"name": "Riverside garden", "drawn_by": "NM", "date": "2026-10-10"})
    cad.result("sheet.create", {"name": "Planting plan", "paper": "a3", "number": "L-101", "title": "Planting plan"})
    cad.result("sheet.create", {"name": "Details", "paper": "a4", "orientation": "portrait", "title": "Details"})
    vp = cad.result("viewport.create", {"sheet": "Planting plan", "scale": 100, "center": [8500, 4000]})
    cad.result("viewport.set_override", {"id": vp["id"], "classes": {"Trees": {"visible": False}}})
    cad.result("viewport.annotate", {"id": vp["id"], "add": [{"kind": "text", "at": [10, 10], "text": "North", "height": 3}]})
    cad.result("title_block.create", {"sheet": "Planting plan", "grid_columns": 6, "grid_rows": 4})
    cad.execute({"op": "add_object", "layer": cad.result("sheet.list")["sheets"][1]["layer"], "shape": {"kind": "text", "at": [20, 20], "text": "DETAIL-A", "height": 5}})
    return cad


@pytest.mark.parametrize("version", ["AC1015", "AC1018", "AC1027", "AC1032"])
def test_the_layouts_open_in_ezdxf_in_each_release(plan, tmp_path, version):
    path = tmp_path / f"{version}.dxf"
    r = plan.result("export.file", {"format": "dxf", "path": str(path), "options": {"version": version}})
    doc = ezdxf.readfile(str(path))
    auditor = doc.audit()
    assert not auditor.errors, [e.message for e in auditor.errors]
    names = [lay.name for lay in doc.layouts if lay.name != "Model"]
    assert names == ["Planting plan", "Details"], names
    first = doc.layouts.get("Planting plan")
    vps = list(first.viewports())
    # The viewport of the paper itself is the first. The user viewport is at 1:100.
    assert len(vps) == 2
    vp = vps[1]
    assert vp.dxf.width == approx(390)
    assert vp.get_scale() == approx(0.01)
    assert "Trees" in vp.frozen_layers
    texts = [t.dxf.text for t in first.query("TEXT")]
    assert "North" in texts
    # The title block: the border and the cells are lines and text on the layout.
    assert len(list(first.query("LWPOLYLINE"))) >= 3
    assert any(t.dxf.text == "Riverside garden" for t in first.query("TEXT"))
    second = doc.layouts.get("Details")
    assert [t.dxf.text for t in second.query("TEXT")] == ["DETAIL-A"]
    # The paper size of the layout: A3 landscape and A4 portrait.
    # A3 is made as 297 x 420 and turned by 90 degrees for a landscape sheet.
    assert (first.dxf.paper_width, first.dxf.paper_height) == approx((297, 420))
    assert first.dxf.plot_rotation == 1
    assert (second.dxf.paper_width, second.dxf.paper_height) == approx((210, 297))
    assert second.dxf.plot_rotation == 0
    assert r["report"]["objects"]["viewport"] == 1


def test_the_sheets_come_back_as_sheets(plan, tmp_path):
    path = tmp_path / "round.dxf"
    plan.result("export.file", {"format": "dxf", "path": str(path)})
    cad2_sheets = plan.result("import.file", {"path": str(path)})
    rep = cad2_sheets["report"]
    assert rep["entities_read"] == rep.get("entities_imported", 0) + rep.get("entities_skipped", 0)
    sheets = plan.result("sheet.list")["sheets"]
    titles = sorted(s["title"] for s in sheets)
    # The original two and the two that the import made.
    assert titles == ["Details", "Details", "Planting plan", "Planting plan"]
    notes = " ".join(rep["notes"])
    assert "layout 'Planting plan' of paper space became a sheet layer" in notes
    assert "layout 'Details' of paper space became a sheet layer" in notes


def test_a_cad_file_with_layouts_becomes_sheets(cad, tmp_path):
    doc = ezdxf.new("R2018", setup=True)
    doc.units = 6
    for name in ("Walls", "Trees", "Annotations"):
        doc.layers.add(name)
    msp = doc.modelspace()
    msp.add_line((0, 0), (100, 0), dxfattribs={"layer": "Walls"})
    msp.add_line((100, 0), (100, 60), dxfattribs={"layer": "Walls"})
    msp.add_circle((50, 30), 5, dxfattribs={"layer": "Trees"})
    layout = doc.layouts.new("A101")
    layout.page_setup(size=(420, 297), margins=(0, 0, 0, 0), units="mm")
    layout.add_text("Site plan", height=6, dxfattribs={"insert": (20, 280), "layer": "Annotations"})
    layout.add_line((10, 10), (410, 10), dxfattribs={"layer": "Annotations"})
    # 1:200, a view of 100 m x 60 m in a frame of 200 x 120 mm... 120 mm * 200 = 24 m of height.
    vp = layout.add_viewport(center=(150, 150), size=(200, 120), view_center_point=(50, 30), view_height=24)
    vp.frozen_layers = ["Trees"]
    layout.add_text("Below the viewport?", height=4, dxfattribs={"insert": (30, 20)})
    path = tmp_path / "layouts.dxf"
    doc.saveas(path)
    r = cad.result("import.file", {"path": str(path)})
    rep = r["report"]
    assert rep["entities_read"] == rep.get("entities_imported", 0) + rep.get("entities_skipped", 0), rep
    assert rep["objects"]["viewport"] == 1
    sheets = cad.result("sheet.list")["sheets"]
    # Layout1 is empty and not imported. A101 is a sheet.
    assert [s["title"] for s in sheets] == ["A101"], sheets
    assert (sheets[0]["width_mm"], sheets[0]["height_mm"]) == (420.0, 297.0)
    vps = cad.result("objects.list", {"kind": "viewport"})
    assert len(vps) == 1
    s = vps[0]["shape"]
    assert s["scale"] == approx(200)
    assert s["min"] == approx([50, 90]) and s["max"] == approx([250, 210])
    assert s["center"] == approx([50000, 30000])
    classes = {c["name"]: c["id"] for c in cad.result("document.summary")["classes"]}
    hidden = [pair[0] for pair in s["class_overrides"] if pair[1].get("visible") is False]
    assert hidden == [classes["Trees"]]
    on_sheet = cad.result("objects.list", {"layer": sheets[0]["layer"]})
    assert [o["shape"]["kind"] for o in on_sheet] == ["text", "line", "viewport", "text"]
