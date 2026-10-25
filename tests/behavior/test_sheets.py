"""Scenario: a designer puts the model on sheets, sets up viewports and a title block, saves views and exports a PDF."""

import re
import zlib

import pytest

from helpers import JsonRpc, rect_cmd, uid

MM = 72.0 / 25.4


@pytest.fixture
def garden(cad):
    """A model with a house, a bed and a tree class, a sheet layer and a project."""
    trees = cad.execute({"op": "add_class", "name": "Trees", "attrs": {"pen": [0, 0, 255, 255], "line_weight": 0.35}})[0]
    house = cad.execute(rect_cmd(0, 0, 12000, 8000, attrs={"pen": [120, 0, 0, 255], "line_weight": 0.5}))[0]
    tree = cad.execute({"op": "add_object", "class": trees, "shape": {"kind": "circle", "center": [15000, 4000], "radius": 2000}})[0]
    cad.result("project.set", {"name": "Riverside garden", "drawn_by": "NM", "date": "2026-10-10"})
    sheet = cad.result("sheet.create", {"name": "Planting plan", "paper": "a3", "number": "L-101", "title": "Planting plan"})["layer"]
    return {"cad": cad, "house": house, "tree": tree, "trees": trees, "sheet": sheet}


def pdf_text(data: bytes) -> str:
    """All text of the content streams: the decoded stream data of every page."""
    out = []
    for m in re.finditer(rb"stream\r?\n(.*?)\r?\nendstream", data, re.S):
        try:
            out.append(zlib.decompress(m.group(1)).decode("latin-1"))
        except zlib.error:
            pass
    return "\n".join(out)


def media_boxes(data: bytes):
    return [[float(x) for x in m.groups()] for m in re.finditer(rb"/MediaBox\s*\[\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\]", data)]


def test_sheet_layers_are_listed_apart_from_design_layers(garden):
    cad = garden["cad"]
    sheets = cad.result("sheet.list")["sheets"]
    assert [(s["number"], s["title"], s["paper"]) for s in sheets] == [("L-101", "Planting plan", "a3")]
    assert (sheets[0]["width_mm"], sheets[0]["height_mm"]) == (420.0, 297.0)
    layers = {l["id"]: l for l in cad.result("document.summary")["layers"]}
    assert layers[garden["sheet"]]["kind"] == "sheet" and layers[uid(1)]["kind"] == "design"
    # The model view does not show paper space.
    cad.execute({"op": "add_object", "layer": garden["sheet"], "shape": {"kind": "line", "a": [0, 0], "b": [100, 0]}})
    assert cad.result("document.summary")["bounds"]["max"] == [17000.0, 8000.0]


def test_a_viewport_fits_the_model_and_follows_changes(garden):
    cad = garden["cad"]
    vp = cad.result("viewport.create", {"sheet": "Planting plan"})
    assert vp["scale"] == 50.0  # 17 m of model in a 390 mm frame
    assert vp["center"] == [8500.0, 4000.0]
    before = cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]
    cad.execute({"op": "add_object", "shape": {"kind": "text", "at": [500, 500], "text": "LIVE-MARK", "height": 400}})
    after = cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]
    assert "LIVE-MARK" not in before and "LIVE-MARK" in after
    # The change is cheap: asking again without a change gives the same drawing.
    assert cad.result("sheet.render", {"sheet": "Planting plan"})["svg"] == after


def test_scale_and_paper_change_what_is_drawn(garden):
    cad = garden["cad"]
    vp = cad.result("viewport.create", {"sheet": "Planting plan", "scale": 100, "center": [8500, 4000]})
    svg = cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]
    # The house is 12 m wide: 120 mm on paper at 1:100.
    assert re.search(r'M [\d.]+ [\d.]+ L 1[0-9]{2}\.\d+', svg) or "120" in svg
    vp = cad.result("viewport.update", {"id": vp["id"], "scale": 50})
    assert vp["scale"] == 50.0
    assert cad.result("sheet.set", {"layer": garden["sheet"], "orientation": "portrait"})["width_mm"] == 297.0


def test_viewport_overrides_do_not_touch_the_document(garden):
    cad = garden["cad"]
    vp = cad.result("viewport.create", {"sheet": "Planting plan"})["id"]
    svg = cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]
    assert "rgb(0,0,255)" in svg
    cad.result("viewport.set_override", {"id": vp, "classes": {"Trees": {"visible": False}}})
    svg = cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]
    assert "rgb(0,0,255)" not in svg
    assert cad.doc()["classes"][1]["visible"] is True
    # A graphic override wins over the class and over the object.
    cad.result("viewport.set_override", {"id": vp, "classes": {"Trees": {"visible": None, "pen": [9, 8, 7, 255], "line_weight": 0.9}}})
    svg = cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]
    assert "rgb(9,8,7)" in svg and 'stroke-width="0.9"' in svg
    cad.result("viewport.set_override", {"id": vp, "layers": {"Layer 1": False}})
    assert "rgb(120,0,0)" not in cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]


def test_annotations_and_crop(garden):
    cad = garden["cad"]
    vp = cad.result("viewport.create", {"sheet": "Planting plan"})["id"]
    r = cad.result("viewport.annotate", {"id": vp, "add": [
        {"kind": "text", "at": [20, 20], "text": "NORTH ARROW", "height": 5},
        {"kind": "dimension", "a": [0, 30], "b": [60, 30], "offset": 6},
    ]})
    assert r["added"] == [1, 2]
    svg = cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]
    assert "NORTH ARROW" in svg
    got = cad.result("viewport.update", {"id": vp, "crop": {"type": "polygon", "points": [[50, 80], [300, 80], [175, 250]]}})
    assert got["crop"]["type"] == "polygon"
    assert "clipPath" in cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]


def test_title_block_fills_fields_from_project_and_sheet(garden):
    cad = garden["cad"]
    cad.result("viewport.create", {"sheet": "Planting plan", "scale": 100})
    tb = cad.result("title_block.create", {"sheet": "Planting plan", "grid_columns": 6, "grid_rows": 4})["id"]
    cells = {c["label"]: c["text"] for c in cad.result("title_block.resolve", {"id": tb})["cells"]}
    assert cells["Project"] == "Riverside garden"
    assert cells["Sheet no."] == "L-101"
    assert cells["Sheet title"] == "Planting plan"
    assert cells["Scale"] == "1:100"
    assert cells["Drawn by"] == "NM" and cells["Date"] == "2026-10-10"
    # The project changes, the title block follows.
    cad.result("project.set", {"name": "Park"})
    assert {c["label"]: c["text"] for c in cad.result("title_block.resolve", {"id": tb})["cells"]}["Project"] == "Park"
    layout = {"width": 100, "height": 20, "cells": [{"x": 0, "y": 0, "w": 100, "h": 20, "label": "Page", "text": "{sheet.index} of {sheet.count}"}]}
    cad.result("title_block.update", {"id": tb, "layout": layout})
    assert cad.result("title_block.resolve", {"id": tb})["cells"][0]["text"] == "1 of 1"
    err = cad.error("title_block.create", {"sheet": "Planting plan", "grid_columns": 99})
    assert err["code"] == 3


def test_saved_views_restore_visibility_and_the_active_layer(garden):
    cad = garden["cad"]
    cad.execute({"op": "add_layer", "name": "Planting"})
    cad.result("saved_view.save", {"name": "All", "zoom": 0.2, "pan": [1000, 2000]})
    cad.execute({"op": "modify_layer", "id": uid(1), "visible": False})
    cad.execute({"op": "modify_class", "id": garden["trees"], "visible": False})
    cad.execute({"op": "set_active", "layer": cad.result("document.summary")["layers"][-1]["id"]})
    restored = cad.result("saved_view.restore", {"name": "all"})
    assert restored["zoom"] == 0.2 and restored["pan"] == [1000.0, 2000.0]
    summary = cad.result("document.summary")
    assert summary["active_layer"] == uid(1)
    assert all(l["visible"] for l in summary["layers"]) and all(c["visible"] for c in summary["classes"])
    assert [v["name"] for v in cad.result("saved_view.list")["views"]] == ["All"]
    cad.result("saved_view.delete", {"name": "All"})
    assert cad.error("saved_view.restore", {"name": "All"})["code"] == 3


def test_export_pdf_has_the_page_size_vector_lines_and_text(garden, tmp_path):
    cad = garden["cad"]
    cad.result("viewport.create", {"sheet": "Planting plan"})
    cad.result("title_block.create", {"sheet": "Planting plan"})
    second = cad.result("sheet.create", {"name": "Detail", "paper": "a4", "orientation": "portrait", "number": "L-201", "title": "Detail sheet"})["layer"]
    cad.execute({"op": "add_object", "layer": second, "shape": {"kind": "text", "at": [20, 20], "text": "DETAIL-NOTE", "height": 5}})
    path = tmp_path / "plans.pdf"
    r = cad.result("export.file", {"format": "pdf", "path": str(path)})
    assert r["pages"] == 2 and r["bytes"] == path.stat().st_size
    data = path.read_bytes()
    assert data.startswith(b"%PDF-")
    boxes = media_boxes(data)
    assert len(boxes) == 2
    assert abs(boxes[0][2] - 420 * MM) < 0.05 and abs(boxes[0][3] - 297 * MM) < 0.05
    assert abs(boxes[1][2] - 210 * MM) < 0.05 and abs(boxes[1][3] - 297 * MM) < 0.05
    text = pdf_text(data)
    for want in ("Riverside garden", "L-101", "Planting plan", "DETAIL-NOTE"):
        assert f"({want})" in text, want
    # Real line weights and curves: the 0.5 mm house outline and the tree circle.
    assert re.search(r"\b1\.41\d* w\b", text)
    assert len(re.findall(r"\bc\n", text)) >= 4
    # One sheet only.
    one = tmp_path / "one.pdf"
    assert cad.result("export.file", {"format": "pdf", "path": str(one), "options": {"sheets": ["Detail"]}})["pages"] == 1
    assert len(media_boxes(one.read_bytes())) == 1


def test_export_pdf_with_a_3d_viewport_embeds_an_image(garden, tmp_path):
    cad = garden["cad"]
    cad.result("viewport.create", {"sheet": "Planting plan", "view": "right_isometric", "dpi": 40, "scale": 200})
    path = tmp_path / "3d.pdf"
    cad.result("export.file", {"format": "pdf", "path": str(path)})
    data = path.read_bytes()
    assert b"/Subtype /Image" in data or b"/Subtype/Image" in data
    assert " Do" in pdf_text(data)


def test_export_svg_and_png_per_sheet(garden, tmp_path):
    cad = garden["cad"]
    cad.result("viewport.create", {"sheet": "Planting plan"})
    svg, png = tmp_path / "s.svg", tmp_path / "s.png"
    cad.result("export.file", {"format": "sheet_svg", "path": str(svg), "options": {"sheet": "Planting plan"}})
    assert svg.read_text(encoding="utf-8").startswith("<svg") and 'width="420mm"' in svg.read_text(encoding="utf-8")
    r = cad.result("export.file", {"format": "sheet_png", "path": str(png), "options": {"sheet": "Planting plan", "dpi": 50.8}})
    assert (r["width"], r["height"]) == (840, 594)
    assert png.read_bytes()[1:4] == b"PNG"


def test_export_refuses_bad_paths_and_missing_sheets(garden, tmp_path):
    cad = garden["cad"]
    assert "path" in cad.error("export.file", {"format": "pdf"})["message"]
    assert ".pdf" in cad.error("export.file", {"format": "pdf", "path": str(tmp_path / "x.txt")})["message"]
    assert cad.error("export.file", {"format": "pdf", "path": str(tmp_path / "no" / "such" / "x.pdf")})["code"] == 4
    assert "not a sheet" in cad.error("export.file", {"format": "pdf", "path": str(tmp_path / "x.pdf"), "options": {"sheets": ["Layer 1"]}})["message"]
    outside = JsonRpc()
    try:
        assert "--allow-dir" in outside.error("export.file", {"format": "pdf", "path": str(tmp_path / "y.pdf")})["message"]
    finally:
        outside.close()
    fresh = JsonRpc(["serve", "--allow-dir", str(tmp_path)])
    try:
        assert "no sheet layers" in fresh.error("export.file", {"format": "pdf", "path": str(tmp_path / "y.pdf")})["message"]
    finally:
        fresh.close()


def test_sheets_survive_save_and_open(garden, tmp_path):
    cad = garden["cad"]
    cad.result("viewport.create", {"sheet": "Planting plan"})
    cad.result("title_block.create", {"sheet": "Planting plan"})
    cad.result("saved_view.save", {"name": "V"})
    path = tmp_path / "drawing.kad"
    assert cad.rpc("file.save", {"path": str(path)})["result"] == {"ok": True, "format": "container"}
    before = cad.doc()
    cad.rpc("file.open", {"path": str(path)})
    assert cad.doc() == before
    assert cad.result("sheet.list")["sheets"][0]["number"] == "L-101"
    assert cad.result("saved_view.list")["views"][0]["name"] == "V"


def test_undo_reverts_sheet_commands(garden):
    cad = garden["cad"]
    cad.result("viewport.create", {"sheet": "Planting plan"})
    assert cad.result("viewport.list")["viewports"]
    cad.result("undo")
    assert cad.result("viewport.list")["viewports"] == []
    cad.result("undo")
    cad.result("undo")
    assert cad.result("sheet.list")["sheets"] == []


def test_describe_and_mcp_cover_the_new_methods(garden):
    cad = garden["cad"]
    names = {m["name"] for m in cad.result("describe")["methods"]}
    for m in ("sheet.create", "viewport.create", "viewport.set_override", "viewport.annotate", "title_block.create", "saved_view.restore", "export.file"):
        assert m in names
    assert "viewport class override" in cad.result("describe")["sheets"]["override_precedence"]


def test_mcp_tools_make_a_sheet_and_export_it(mcp, tmp_path):
    tools = {tool["name"]: tool for tool in mcp.request("tools/list")["result"]["tools"]}
    methods = lambda name: set(tools[name]["inputSchema"]["properties"]["method"]["enum"])
    assert {"sheet.create", "viewport.create", "viewport.set_override", "viewport.annotate", "title_block.create"} <= methods("sheets_print")
    assert "saved_view.save" in methods("document") and "export.file" in methods("io")
    mcp.method_json("document", "execute", {"command": rect_cmd(0, 0, 5000, 3000)})
    layer = mcp.method_json("sheets_print", "sheet.create", {"name": "A", "paper": "a4"})["layer"]
    assert mcp.method_json("sheets_print", "viewport.create", {"sheet": layer})["view"] == "plan"
    path = tmp_path / "mcp.pdf"
    assert mcp.method_json("io", "export.file", {"format": "pdf", "path": str(path)})["pages"] == 1
    assert path.read_bytes().startswith(b"%PDF-")
    # A rendered sheet comes back as an image the model can look at.
    result = mcp.method("sheets_print", "sheet.render", {"sheet": layer, "format": "png", "dpi": 20})
    assert result["content"][0]["type"] == "image" and result["content"][0]["mimeType"] == "image/png"


def test_estonian_text_survives_in_the_pdf(garden, tmp_path):
    from helpers import pdftotext

    cad = garden["cad"]
    second = cad.result("sheet.create", {"name": "Eesti", "number": "E-1", "title": "Šõõr Žürii äöü"})["layer"]
    cad.execute({"op": "add_object", "layer": second, "shape": {"kind": "text", "at": [20, 20], "text": "Šõõr Žürii äöü \u2013 \u201cšž\u201d", "height": 6}})
    path = tmp_path / "ee.pdf"
    cad.result("export.file", {"format": "pdf", "path": str(path), "options": {"sheets": [second]}})
    text = pdftotext(path)
    assert "Šõõr Žürii äöü" in text
    assert "\u201cšž\u201d" in text and "\u2013" in text


def test_viewport_visibility_does_not_follow_the_document(garden):
    cad = garden["cad"]
    cad.result("viewport.create", {"sheet": "Planting plan"})
    cad.execute({"op": "modify_class", "id": garden["trees"], "visible": False})
    assert "rgb(0,0,255)" in cad.result("sheet.render", {"sheet": "Planting plan"})["svg"]


def test_plants_and_exports_leave_paper_space_alone(garden, tmp_path):
    cad = garden["cad"]
    err = cad.error("execute", {"command": {"op": "add_plants", "positions": [[1, 1]], "layer": garden["sheet"]}})
    assert "design layer" in err["message"]
    cad.execute({"op": "add_object", "layer": garden["sheet"], "shape": {"kind": "text", "at": [11, 11], "text": "PAPER-ONLY", "height": 5}})
    cad.result("viewport.create", {"sheet": "Planting plan"})
    dxf = tmp_path / "model.dxf"
    cad.result("export.file", {"format": "dxf", "path": str(dxf), "options": {"sheets": False}})
    text = dxf.read_text(encoding="utf-8")
    assert "PAPER-ONLY" not in text and "Planting plan" not in text
    # By default the sheet is a layout of paper space (IOP-16).
    with_sheets = tmp_path / "with_sheets.dxf"
    cad.result("export.file", {"format": "dxf", "path": str(with_sheets)})
    text = with_sheets.read_text(encoding="utf-8")
    assert "PAPER-ONLY" in text and "Planting plan" in text
