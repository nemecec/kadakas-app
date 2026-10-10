"""Scenario: a designer dimensions, tags and labels a plan, then prints it."""

import shutil
import subprocess

import pytest

from helpers import rect_cmd


def line_cmd(a, b):
    return {"op": "add_object", "shape": {"kind": "line", "a": a, "b": b}}


def annotations(cad, **p):
    return {i["id"]: i for i in cad.result("annotation.list", p)["items"]}


def test_dimension_follows_the_object_and_stays_when_it_is_deleted(cad):
    ln = cad.execute(line_cmd([0, 0], [5000, 0]))[0]
    dim = cad.result("dimension.add", {"mode": "horizontal", "anchors": [{"object": ln, "vertex": 0}, {"object": ln, "vertex": 1}]})
    assert dim["texts"] == ["5000"] and dim["values"] == [5000.0]
    cad.execute({"op": "modify_object", "id": ln, "shape": {"kind": "line", "a": [0, 0], "b": [8000, 0]}})
    assert annotations(cad)[dim["id"]]["computed"]["texts"] == ["8000"]
    cad.execute({"op": "move_objects", "ids": [ln], "delta": [100, 50]})
    assert annotations(cad)[dim["id"]]["computed"]["points"] == [[100.0, 50.0], [8100.0, 50.0]]
    cad.execute({"op": "delete_objects", "ids": [ln]})
    kept = annotations(cad)[dim["id"]]
    assert kept["computed"]["points"] == [[100.0, 50.0], [8100.0, 50.0]]
    assert "anchors" not in kept["annotation"]
    assert cad.result("undo")["done"]
    assert len(annotations(cad)[dim["id"]]["annotation"]["anchors"]) == 2


def test_all_dimension_modes(cad):
    circle = cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 1000}})[0]
    arc = cad.execute({"op": "add_object", "shape": {"kind": "arc", "center": [0, 0], "radius": 1000, "start": 0, "end": 1.5707963267948966}})[0]
    cases = [
        ({"mode": "vertical", "points": [[0, 0], [0, 3000]]}, "3000"),
        ({"mode": "aligned", "points": [[0, 0], [3000, 4000]]}, "5000"),
        ({"mode": "chain_horizontal", "points": [[0, 0], [1000, 0], [3000, 0]]}, ["1000", "2000"]),
        ({"mode": "baseline_horizontal", "points": [[0, 0], [1000, 0], [3000, 0]]}, ["1000", "3000"]),
        ({"mode": "ordinate_vertical", "points": [[0, 0], [100, 400]]}, "400"),
        ({"mode": "angular", "points": [[0, 0], [1000, 0], [0, 1000]]}, "90.0°"),
        ({"mode": "radial", "object": circle}, "R1000"),
        ({"mode": "diameter", "object": circle}, "Ø2000"),
        ({"mode": "arc_length", "object": arc}, "Arc 1571"),
    ]
    for params, want in cases:
        texts = cad.result("dimension.add", params)["texts"]
        assert texts == (want if isinstance(want, list) else [want]), params
    assert "unknown mode" in cad.error("dimension.add", {"mode": "wavy", "points": [[0, 0], [1, 1]]})["message"]


def test_dimension_style_changes_the_text(cad):
    st = cad.result("dimension_style.create", {"name": "Metric", "unit": "m", "precision": 2, "show_unit": True})["id"]
    d = cad.result("dimension.add", {"mode": "horizontal", "points": [[0, 0], [2500, 0]], "style": "Metric"})
    assert d["texts"] == ["2.50 m"]
    cad.result("dimension_style.update", {"id": st, "precision": 1})
    assert annotations(cad)[d["id"]]["computed"]["texts"] == ["2.5 m"]
    assert "used by" in cad.error("dimension_style.delete", {"id": st})["message"]


def test_data_tags_legend_and_tag_all(cad):
    style = cad.result("plant_style.create", {"name": "Oak", "id_code": "QR", "spread": 3000, "height": 8000})["id"]
    cad.execute({"op": "add_plants", "style": style, "positions": [[1000, 1000], [6000, 1000]]})
    cad.result("tag_style.create", {"name": "Plant tag", "lines": ["{IdCode} {Kind}", "{ROUND(Area, 1)} m2"], "frame": "box"})
    r = cad.result("data_tag.tag_all", {"style": "Plant tag", "criteria": 'Kind = "plant"'})
    assert r["tagged"] == 2
    again = cad.result("data_tag.tag_all", {"style": "Plant tag", "criteria": 'Kind = "plant"'})
    assert again["tagged"] == 0
    tags = annotations(cad, type="data_tag")
    lines = [t["computed"]["lines"] for t in tags.values()]
    assert lines[0][0] == "QR plant" and lines[0][1].endswith("m2")
    legend = cad.result("legend.add", {"at": [0, -5000], "title": "Plants", "criteria": 'Kind = "plant"', "group_by": "Style"})
    assert legend["rows"] == [{"label": "Oak", "count": 2}]
    cad.execute({"op": "add_plants", "style": style, "positions": [[9000, 1000]]})
    assert annotations(cad)[legend["id"]]["computed"]["rows"][0]["count"] == 3
    # Deleting a tagged plant removes its tag.
    plant_ids = [o["id"] for o in cad.result("objects.list", {"kind": "plant"})]
    cad.execute({"op": "delete_objects", "ids": plant_ids[:1]})
    assert len(annotations(cad, type="data_tag")) == 1
    svg = cad.result("render.svg", {"width": 600})["svg"]
    assert 'data-type="data_tag"' in svg and 'data-type="legend"' in svg


def test_scale_bar_follows_the_scale(cad):
    cad.execute({"op": "modify_layer", "id": 1, "scale": 50})
    bar = cad.result("scale_bar.add", {"at": [0, 0], "segment": 5, "segments": 4})
    assert bar["length_mm"] == 20000 and bar["length_paper_mm"] == 400
    own = cad.result("scale_bar.add", {"at": [0, 0], "segment": 10, "segments": 1, "scale": 200})
    assert own["length_paper_mm"] == pytest.approx(50.0)
    sheet = cad.result("sheet.create", {"name": "S", "paper": "a3"})["layer"]
    vp = cad.result("viewport.create", {"sheet": sheet, "scale": 250, "view": "plan"})["id"]
    inside = cad.result("scale_bar.add", {"at": [20, 20], "viewport": vp})
    assert inside["length_paper_mm"] == pytest.approx(80.0)


def test_callout_with_notes_database_and_north_arrow_and_revisions(cad):
    ln = cad.execute(line_cmd([0, 0], [1000, 0]))[0]
    cad.result("notes_db.create", {"name": "General", "notes": {"N1": "Verify on site"}})
    c = cad.result("callout.add", {"at": [3000, 3000], "leaders": [{"object": ln, "vertex": 1}], "note": "N1", "notes_db": "General"})
    assert c["text"] == "Verify on site"
    cad.result("notes_db.update", {"id": "General", "notes": {"N1": "Changed"}})
    assert annotations(cad)[c["id"]]["computed"]["text"] == "Changed"
    cad.execute({"op": "move_objects", "ids": [ln], "delta": [0, 500]})
    assert annotations(cad)[c["id"]]["annotation"]["leaders"][0]["points"][0] == [1000.0, 500.0]
    assert cad.result("north_arrow.add", {"at": [0, 0], "rotation_deg": 15})["rotation_deg"] == 15
    cad.result("revision_cloud.add", {"points": [[0, 0], [1000, 0], [1000, 1000]], "revision": "A", "description": "Moved path"})
    cad.result("revision_cloud.add", {"points": [[0, 0], [500, 0], [500, 500]], "revision": "A"})
    revs = cad.result("revision.list")["revisions"]
    assert len(revs) == 1 and len(revs[0]["clouds"]) == 2 and revs[0]["description"] == "Moved path"


def test_update_apply_text_style_and_errors(cad):
    t = cad.result("annotation.text", {"at": [0, 0], "text": "Hello"})["id"]
    cad.result("text_style.create", {"name": "Big", "size_mm": 6})
    assert cad.result("annotation.apply_text_style", {"ids": [t], "style": "Big"})["changed"] == 1
    assert annotations(cad)[t]["annotation"]["style"]
    updated = cad.result("annotation.update", {"id": t, "text": "Bye"})
    assert updated["computed"]["text"] == "Bye"
    assert cad.error("annotation.update", {"id": 9999})
    assert cad.error("annotation.list", {"type": "bogus"})
    assert cad.error("data_tag.add", {"style": "nope", "object": 1})
    assert cad.error("tag_style.create", {"name": "bad", "lines": ["{open"]})
    assert cad.result("tag_style.list")["items"] == []


def test_dxf_export_has_annotation_lines_and_text(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf")
    cad.result("dimension.add", {"mode": "horizontal", "points": [[0, 0], [5000, 0]]})
    path = tmp_path / "a.dxf"
    cad.result("export.file", {"format": "dxf", "path": str(path)})
    doc = ezdxf.readfile(path)
    texts = [e.dxf.text for e in doc.modelspace().query("TEXT")]
    assert "5000" in texts
    assert len(doc.modelspace().query("LWPOLYLINE")) >= 3


@pytest.mark.skipif(shutil.which("pdftotext") is None, reason="pdftotext is not installed")
def test_pdf_has_dimension_tag_and_scale_bar_text(cad, tmp_path):
    cad.execute(rect_cmd(0, 0, 12000, 8000))
    sheet = cad.result("sheet.create", {"name": "Plan", "paper": "a3", "number": "L-101", "title": "Plan"})["layer"]
    vp = cad.result("viewport.create", {"sheet": sheet, "scale": 100, "view": "plan"})["id"]
    cad.result("dimension.add", {"mode": "horizontal", "points": [[20, 20], [60, 20]], "viewport": vp})
    cad.result("scale_bar.add", {"at": [20, 5], "segment": 5, "segments": 2, "viewport": vp})
    cad.result("annotation.text", {"at": [20, 60], "text": "Planting notes", "layer": sheet})
    path = tmp_path / "a.pdf"
    cad.result("export.file", {"format": "pdf", "path": str(path)})
    text = subprocess.run(["pdftotext", "-layout", str(path), "-"], capture_output=True, text=True, check=True).stdout
    assert "4000" in text, text  # 40 paper mm at 1:100
    assert "10 m" in text and "Planting notes" in text
