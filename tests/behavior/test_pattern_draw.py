"""Scenario: an agent iterates on a parking lot while a person watches the drawing. What the
drawing shows is what the report counts and the export writes (feature spec PAT-12): zones, courses
and their own patterns are in `render.svg`; a render of part of a big lot shows its real units; a
view with too many units is a preview that says so; the findings of the check can be marked."""

import re
from xml.etree import ElementTree

SVG = "{http://www.w3.org/2000/svg}"
LABEL = "preview: zoom in for the real pattern"


def lot(cad, w, h, name="P"):
    cad.result(
        "hardscape_style.create",
        {"name": name, "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5}},
    )
    cad.result(
        "hardscape.add",
        {"boundary": [[0, 0], [w, 0], [w, h], [0, h]], "style": name, "origin": [0, 0], "angle": 0},
    )
    return cad.result("objects.list", {"kind": "hardscape"})[0]["id"]


def drawn_units(svg):
    """(whole, cut) units that an SVG draws, per group name: the subpaths of the whole-unit path
    and the cut-unit paths. The group says the same numbers in its attributes."""
    root = ElementTree.fromstring(svg)
    out = {}
    for g in root.iter(SVG + "g"):
        if g.get("data-part") != "group":
            continue
        whole = sum(
            p.get("d").count("M ") for p in g.iter(SVG + "path") if p.get("data-part") == "units-whole"
        )
        cut = sum(1 for p in g.iter(SVG + "path") if p.get("data-part") == "unit-cut")
        assert (int(g.get("data-whole")), int(g.get("data-cut"))) == (whole, cut)
        out[g.get("data-group")] = (whole, cut)
    return out


def test_the_drawing_has_the_units_of_the_report_with_zones_and_courses(cad):
    hid = lot(cad, 10_000, 6_000)
    cad.result(
        "hardscape.course",
        {"id": hid, "courses": [{"name": "Kerb", "path": [[0, 0], [10_000, 0]], "side": "left"}]},
    )
    cad.result("hardscape.split", {"id": hid, "lines": [[[6000, -50], [6000, 6050]]]})
    cad.result("hardscape.split", {"id": hid, "zones": [{"index": 0, "name": "Stalls"}]})
    row = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
    svg = cad.result("render.svg", {"width": 1200})["svg"]
    drawn = drawn_units(svg)
    assert {g["name"] for g in row["groups"]} == set(drawn)
    for g in row["groups"]:
        assert drawn[g["name"]] == (g["whole_units"], g["cut_units"]), g["name"]
    assert sum(w for w, _ in drawn.values()) == row["whole_units"]
    assert sum(c for _, c in drawn.values()) == row["cut_units"]
    # The zone edges are a thin dashed line, and the cut units have a light fill.
    assert 'data-part="zone-edges"' in svg and "stroke-dasharray" in svg
    assert 'fill-opacity="0.18"' in svg and 'data-detail="full"' in svg
    assert LABEL not in svg
    # The same call twice gives the same picture.
    assert cad.result("render.svg", {"width": 1200})["svg"] == svg


def test_a_render_of_part_of_a_big_lot_shows_the_real_units(cad):
    lot(cad, 100_000, 50_000)  # 5,000 m2: about 233,000 units
    whole = cad.result("render.svg", {"width": 900})["svg"]
    assert 'data-detail="preview"' in whole and LABEL in whole
    assert 'data-part="units-whole"' not in whole and 'data-part="joints"' in whole
    assert len(whole) < 2_000_000
    region = [40_000, 20_000, 42_000, 21_000]
    zoom = cad.result("render.svg", {"width": 900, "region": region})["svg"]
    root = ElementTree.fromstring(zoom)
    # The picture is the region (the y axis of the picture points down).
    assert [float(x) for x in root.get("viewBox").split()] == [40_000, -21_000, 2_000, 1_000]
    assert 'data-detail="full"' in zoom and LABEL not in zoom
    ((whole_n, cut_n),) = drawn_units(zoom).values()
    # 2 m x 1 m over cells of 205 x 105 mm: about 93 units, and the ones on the border of the region.
    assert 80 <= whole_n + cut_n <= 140 and len(zoom) < 100_000
    # A region outside the lot has nothing.
    away = cad.result("render.svg", {"region": [-9000, -9000, -8000, -8000]})["svg"]
    assert "units-whole" not in away
    for bad in ([0, 0, 0, 10], [0, 0, 1], "x"):
        assert cad.error("render.svg", {"region": bad})["code"] == -32602


def test_the_findings_of_the_check_can_be_marked_in_the_drawing(cad):
    # A strip of 3 mm is left at the right edge: small pieces.
    hid = lot(cad, 8003, 400)
    assert 'data-part="check"' not in cad.result("render.svg", {"width": 1000})["svg"]
    svg = cad.result("render.svg", {"width": 1000, "show_check": True})["svg"]
    ElementTree.fromstring(svg)
    chk = cad.result("unit_pattern.check", {"ids": [hid]})["objects"][0]
    assert chk["findings"]
    marks = re.findall(r'data-part="finding" data-kind="(\w+)"', svg)
    assert sorted(marks) == sorted(f["kind"] for f in chk["findings"])


def test_the_3d_picture_says_that_it_does_not_draw_the_pattern(cad):
    """A paving texture on a hardscape with a joint pattern has lines that are not its joints: the
    3D picture leaves the texture out and says so; render.svg draws the pattern."""
    hid = lot(cad, 4000, 2500)
    cad.result("material.create", {"name": "Paving", "base_color": [180, 180, 175], "texture": {"procedural": "pavers"}})
    cad.result("material.assign", {"material": "Paving", "object": hid})
    png = cad.result("render.png", {"width": 120, "height": 90, "view": "top"})
    assert any("pattern not drawn in 3D" in n for n in png["notes"]), list(png)
    # A slab without a pattern keeps the texture, and nothing is said.
    cad.result("material.assign", {"material": None, "object": hid})
    cad.result("hardscape.add", {"boundary": [[6000, 0], [9000, 0], [9000, 2000], [6000, 2000]]})
    other = [o["id"] for o in cad.result("objects.list", {"kind": "hardscape"}) if o["id"] != hid][0]
    cad.result("material.assign", {"material": "Paving", "object": other})
    png = cad.result("render.png", {"width": 120, "height": 90, "view": "top"})
    assert "notes" not in png
