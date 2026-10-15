"""Scenario: an agent reads a parking lot from DXF, lays a paver pattern on it, checks the picture
and the counts, and writes a DXF for the contractor (feature spec PAT-01 to PAT-05, HRD-12, HRD-13).

The fixture `fixtures/parking_lot.dxf` has the lot outline and a planting island on layer LOT,
stall lines on STALLS, and a text. The coordinates are large map numbers.
"""

import base64
import math

import pytest

from helpers import REPO

FIXTURE = REPO / "tests" / "behavior" / "fixtures" / "parking_lot.dxf"
ISLAND = [(22.0, 14.0), (30.0, 14.0), (31.0, 20.0), (26.0, 24.0), (21.0, 20.0)]
ORIGIN = (650_000_000, 6_470_000_000)


def shoelace(points):
    x0, y0 = points[0]
    pts = [(x - x0, y - y0) for x, y in points]
    return abs(sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(pts, pts[1:] + pts[:1]))) / 2


ISLAND_M2 = shoelace(ISLAND)
LOT_M2 = 60 * 40 - 12 * 12 / 2


def edges(points):
    return [math.dist(a, b) for a, b in zip(points, points[1:] + points[:1])]


def rect(w, h):
    return [[0, 0], [w, 0], [w, h], [0, h]]


def add_poly(cad, points):
    return cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": True, "points": points}})[0]


def style(cad, name="Pavers 200x100", kind="running_bond", gap=5, border=None, **joints):
    params = {
        "name": name,
        "material": "Concrete pavers",
        "thickness": 80,
        "joints": {"kind": kind, "unit_length": 200, "unit_width": 100, "spacing": gap, **joints},
    }
    if border:
        params["border"] = {"width": border, "material": "Kerb"}
    return cad.result("hardscape_style.create", params)["id"]


def lay_the_lot(cad, **style_args):
    cad.result("import.file", {"path": str(FIXTURE)})
    classes = {c["name"]: c["id"] for c in cad.result("document.summary")["classes"]}
    polys = cad.result("objects.list", {"kind": "polyline", "class": classes["LOT"]})
    assert len(polys) == 2 and all(p["shape"]["closed"] for p in polys)
    style(cad, **style_args)
    made = cad.result(
        "hardscape.from_shapes",
        {"ids": [p["id"] for p in polys], "style": "Pavers 200x100", "holes_from_inner": True},
    )
    assert len(made["created"]) == 1 and made["created"][0]["holes"] == 1
    assert len(made["holes"]) == 1 and made["skipped"] == []
    hid = made["created"][0]["id"]
    cad.result("hardscape.set", {"id": hid, "align_to_edge": 0})
    return hid


def polylines_by_layer(dxf):
    by_layer = {}
    for e in dxf.modelspace():
        if e.dxftype() == "LWPOLYLINE":
            by_layer.setdefault(e.dxf.layer, []).append(([(p[0], p[1]) for p in e.get_points()], e))
    return by_layer


def test_agent_lays_pavers_on_a_parking_lot_and_exports_the_units(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    hid = lay_the_lot(cad)
    # The sources stay (delete_source is off by default); the hardscape has the island as a hole.
    obj = cad.result("objects.get", {"id": hid})
    hs = obj["data"]["hardscape"]
    assert len(hs["holes"]) == 1 and hs["angle"] == 0.0 and hs["origin"] is not None
    assert len(cad.result("objects.list", {"kind": "polyline"})) == 2
    png = base64.b64decode(cad.result("render.png", {"view": "top", "width": 320, "height": 240})["png_base64"])
    assert png[:8] == b"\x89PNG\r\n\x1a\n"

    rep = cad.result("report.unit_pattern", {"waste_percent": 5, "gap_length": True})
    row = rep["rows"][0]
    assert row["id"] == hid and rep["totals"]["complete"] is True
    assert row["region_area_m2"] == pytest.approx(LOT_M2 - ISLAND_M2, abs=0.01)
    assert row["whole_units"] > 90_000 and row["cut_units"] > 100
    assert row["order_units"] == row["whole_units"] + row["cut_units"] == row["order_units_max"]
    assert row["order_units_min"] <= row["order_units"]
    assert row["order_units_with_waste"] == -(-row["order_units"] * 105 // 100)
    assert rep["totals"]["order_units_with_waste"] == row["order_units_with_waste"]
    assert row["area_laid_m2"] + row["gap_area_m2"] == pytest.approx(row["region_area_m2"], abs=1e-6)
    assert row["slivers"] <= row["cut_units"] and row["unfillable_pieces"] <= row["cut_units"]
    assert row["gap_length_m"] > 1000
    assert row["by_unit"][0]["name"] == "unit" and row["by_unit"][0]["whole_units"] == row["whole_units"]
    # Without gap_length nothing is measured.
    assert cad.result("report.unit_pattern")["rows"][0]["gap_length_m"] is None
    # report.hardscape carries the same counts, and the area without the island.
    hrow = cad.result("report.hardscape")["rows"][0]
    assert hrow["pattern"]["whole_units"] == row["whole_units"]
    assert hrow["plan_area_m2"] == pytest.approx(LOT_M2 - ISLAND_M2, abs=0.01)

    out = tmp_path / "paving.dxf"
    r = cad.result("export.file", {"path": str(out), "options": {"patterns": "units"}})
    assert any("whole" in n and "cut" in n for n in r["report"]["notes"])
    dxf = ezdxf.readfile(str(out))
    auditor = dxf.audit()
    assert not auditor.errors, [e.message for e in auditor.errors]
    assert not auditor.fixes, [e.message for e in auditor.fixes]
    from ezdxf import recover

    _, rec = recover.readfile(str(out))
    assert not rec.errors and not rec.fixes
    by_layer = polylines_by_layer(dxf)
    whole = [p for p, _ in by_layer["LOT-UNITS-WHOLE"]]
    cut = [p for p, _ in by_layer["LOT-UNITS-CUT"]]
    assert (len(whole), len(cut)) == (row["whole_units"], row["cut_units"])
    assert len(by_layer["LOT"]) == 3, "the outline of the hardscape and the two source outlines"
    # Every whole unit is exactly 200 x 100 mm (the gap is between the units).
    for u in whole[:2000]:
        a, b = edges(u)[:2]
        assert sorted([round(a, 3), round(b, 3)]) == [100.0, 200.0], u
    # Real-world coordinates, and the island is empty.
    assert min(p[0] for u in whole for p in u) > ORIGIN[0] - 1
    area = sum(shoelace(u) for u in whole + cut) / 1e6
    assert area == pytest.approx(row["area_laid_m2"], abs=1e-3)
    island = [(ORIGIN[0] + x * 1000, ORIGIN[1] + y * 1000) for x, y in ISLAND]
    cx = sum(p[0] for p in island) / 5
    cy = sum(p[1] for p in island) / 5
    assert not any(
        min(p[0] for p in u) < cx < max(p[0] for p in u) and min(p[1] for p in u) < cy < max(p[1] for p in u)
        for u in whole + cut
    ), "no unit on the island"

    # Gap lines are an option of their own, and the default writes no pattern.
    gaps = tmp_path / "gaps.dxf"
    cad.result("export.file", {"path": str(gaps), "options": {"patterns": "gaps"}})
    layers = {e.dxf.layer for e in ezdxf.readfile(str(gaps)).modelspace()}
    assert "LOT-GAPS" in layers and "LOT-UNITS-WHOLE" not in layers
    plain = tmp_path / "plain.dxf"
    cad.result("export.file", {"path": str(plain)})
    assert not any("-UNITS-" in e.dxf.layer or "-GAPS" in e.dxf.layer for e in ezdxf.readfile(str(plain)).modelspace())


def test_a_small_known_case_has_exact_counts(cad):
    r = cad.result(
        "unit_pattern.layout",
        {
            "region": {"outline": rect(2050, 1050)},
            "pattern": {"bond": "stack", "unit_length": 200, "unit_width": 100, "gap": 5},
            "origin": [0, 0],
            "include": ["units"],
            "limit": 0,
        },
    )
    s = r["summary"]
    assert (s["whole_units"], s["cut_units"], s["slivers"], s["order_units"]) == (100, 0, 0, 100)
    assert s["order_units_min"] == 100 and s["order_units_max"] == 100
    assert s["area_laid_m2"] == pytest.approx(100 * 0.2 * 0.1)
    assert s["gap_area_m2"] == pytest.approx(2.05 * 1.05 - 2.0)
    assert s["widest_joint_mm"] == 5
    assert r["units_total"] == 100 and r["units"] == [] and r["next_offset"] is None
    # 5 percent waste on 100 units is 105; ten units with 10 percent are 11, not 12.
    s = cad.result("report.unit_pattern", {"waste_percent": 5})["totals"]
    assert s["order_units"] == 0
    r = cad.result(
        "unit_pattern.layout",
        {"region": {"outline": rect(2050, 1050)}, "pattern": {"bond": "stack", "unit_length": 200, "unit_width": 100, "gap": 5},
         "origin": [0, 0], "include": ["units"], "limit": 3},
    )
    assert len(r["units"]) == 3 and r["next_offset"] == 3


@pytest.mark.parametrize("bond", ["running_bond", "stack", "herringbone", "basket_weave"])
def test_units_keep_their_size_in_every_bond_with_a_gap(cad, bond):
    r = cad.result(
        "unit_pattern.layout",
        {
            "region": {"outline": rect(4100, 2100)},
            "pattern": {"bond": bond, "unit_length": 200, "unit_width": 100, "gap": 5},
            "origin": [0, 0],
            "angle": 25,
            "include": ["units"],
            "limit": 20000,
        },
    )
    whole = [u for u in r["units"] if u["whole"]]
    assert len(whole) > 100
    for u in whole:
        a, b = edges([tuple(p) for p in u["outline"]])[:2]
        assert sorted([round(a, 6), round(b, 6)]) == [100.0, 200.0], (bond, u["outline"])
        assert u["area_m2"] == pytest.approx(0.02, abs=1e-9)


def test_herringbone_of_300_by_200_keeps_300_by_200(cad):
    r = cad.result(
        "unit_pattern.layout",
        {"region": {"outline": rect(6000, 4000)}, "pattern": {"bond": "herringbone", "unit_length": 300, "unit_width": 200, "gap": 6},
         "include": ["units"], "limit": 20000},
    )
    whole = [u for u in r["units"] if u["whole"]]
    assert whole and all(
        sorted(round(e, 6) for e in edges([tuple(p) for p in u["outline"]])[:2]) == [200.0, 300.0] for u in whole
    )


def test_an_island_touching_the_kerb_and_an_arc_edge(cad):
    # A lot with an arc side (a half disc) and an island that sits on the flat side, closed with a repeated point.
    lot = cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": True, "points": [
        [-2000, 0], {"p": [0, 2000], "kind": "point_on_arc"}, [2000, 0]]}})[0]
    island = add_poly(cad, [[-300, 0], [300, 0], [300, 400], [-300, 400], [-300, 0]])
    style(cad)
    made = cad.result("hardscape.from_shapes", {"ids": [lot, island], "style": "Pavers 200x100"})
    assert made["created"][0]["holes"] == 1 and made["skipped"] == []
    hid = made["created"][0]["id"]
    assert len(cad.result("objects.get", {"id": hid})["data"]["hardscape"]["holes"][0]) == 4
    row = cad.result("report.unit_pattern")["rows"][0]
    want = math.pi * 2.0 * 2.0 / 2 - 0.6 * 0.4
    assert row["region_area_m2"] == pytest.approx(want, rel=0.002)
    assert row["area_laid_m2"] + row["gap_area_m2"] == pytest.approx(row["region_area_m2"], abs=1e-6)
    assert cad.result("report.hardscape")["rows"][0]["plan_area_m2"] == pytest.approx(want, rel=0.002)
    # A second run: two lots that overlap in part are never both made.
    a = add_poly(cad, rect(4000, 2000))
    b = add_poly(cad, [[3000, 500], [6000, 500], [6000, 1500], [3000, 1500]])
    made = cad.result("hardscape.from_shapes", {"ids": [a, b], "style": "Pavers 200x100", "holes_from_inner": False})
    assert len(made["created"]) == 1 and made["skipped"][0]["id"] == b and "overlaps" in made["skipped"][0]["reason"]


def test_the_kerb_border_is_not_paved_and_the_numbers_agree(cad):
    style(cad, border=150)
    hid = cad.result("hardscape.add", {"boundary": rect(10000, 6000), "style": "Pavers 200x100"})["id"]
    cad.result("hardscape.set", {"id": hid, "align_to_edge": 0})
    o = cad.result("objects.get", {"id": hid})["data"]["hardscape"]
    assert o["origin"] == [150.0, 150.0], "the pattern starts on the inner border line"
    row = cad.result("report.unit_pattern")["rows"][0]
    inner = (10 - 0.3) * (6 - 0.3)
    assert row["region_area_m2"] == pytest.approx(inner)
    assert row["area_laid_m2"] + row["gap_area_m2"] == pytest.approx(inner, abs=1e-6)
    hs = cad.result("report.hardscape")["rows"][0]
    surface = next(layer for layer in hs["layers"] if layer["kind"] == "surface")
    kerb = next(layer for layer in hs["layers"] if layer["kind"] == "border")
    assert surface["area_m2"] == pytest.approx(inner) and kerb["area_m2"] == pytest.approx(60 - inner)
    # Every unit lies inside the border.
    r = cad.result("unit_pattern.layout", {"id": hid, "include": ["units"], "limit": 20000})
    xs = [p[0] for u in r["units"] for p in u["outline"]]
    ys = [p[1] for u in r["units"] for p in u["outline"]]
    assert min(xs) >= 150 - 1e-6 and max(xs) <= 9850 + 1e-6 and min(ys) >= 150 - 1e-6 and max(ys) <= 5850 + 1e-6


def test_pattern_layout_and_to_objects_on_the_lot(cad):
    hid = lay_the_lot(cad)
    r = cad.result("unit_pattern.layout", {"id": hid, "include": ["units"], "limit": 5})
    assert r["summary"]["whole_units"] > 90_000 and len(r["units"]) == 5 and r["next_offset"] == 5
    before = cad.result("document.summary")["object_count"]
    made = cad.result(
        "unit_pattern.to_objects",
        {
            "region": {"outline": [[0, 0], [2050, 0], [2050, 1050], [0, 1050]]},
            "pattern": {"unit_length": 200, "unit_width": 100, "gap": 5, "bond": "stack"},
            "origin": [0, 0],
            "name": "Test units",
        },
    )
    assert made["units"] == 100 and made["group"]
    assert cad.result("document.summary")["object_count"] == before + 101
    cad.result("undo")
    assert cad.result("document.summary")["object_count"] == before


def test_the_export_limit_is_an_error_that_names_the_hardscape(cad, tmp_path):
    style(cad, name="P", kind="stack")
    big = cad.result("hardscape.add", {"boundary": rect(200000, 150000), "style": "P"})["id"]
    err = cad.error("export.file", {"path": str(tmp_path / "x.dxf"), "options": {"patterns": "units"}})
    assert big in err["message"] and "limit" in err["message"]
    assert not (tmp_path / "x.dxf").exists()
    ok = cad.result("export.file", {"path": str(tmp_path / "ok.dxf"), "options": {"patterns": "none"}})
    assert ok["exported"] >= 1
    # The report names the same hardscape in an error row, and says the totals are not complete.
    rep = cad.result("report.unit_pattern")
    assert rep["rows"][0]["id"] == big and "limit" in rep["rows"][0]["error"]
    assert rep["totals"]["complete"] is False
    # A bad option is an error, not a silent default.
    assert "sliver_share" in cad.error("report.unit_pattern", {"sliver_share": 3})["message"]
    assert "unit_length" in cad.error("unit_pattern.layout", {"region": {"outline": rect(1000, 1000)}})["message"]


def test_modules_hexagon_and_i_shape_on_the_parking_lot(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    hid = lay_the_lot(cad)
    hexagon = cad.result("unit_pattern.module_preset", {"preset": "hexagon", "across_flats": 200})["pattern"]
    i_shape = cad.result("unit_pattern.module_preset", {"preset": "i_shape", "length": 240, "width": 165, "notch": 25})["pattern"]
    assert hexagon["kind"] == "module" and i_shape["units"][0]["name"] == "i_shape"
    for name, module in (("hex", hexagon), ("ishape", i_shape)):
        # Restyle the hardscape with the module as its joint pattern.
        cad.result("hardscape_style.update", {"id": "Pavers 200x100", "joints": {**module, "spacing": 4}})
        row = cad.result("report.unit_pattern")["rows"][0]
        assert row["bond"] == "module" and row["whole_units"] > 1000, name
        assert row["area_laid_m2"] + row["gap_area_m2"] == pytest.approx(row["region_area_m2"], abs=1e-6)
        assert row["unit_length_mm"] is None
        out = tmp_path / f"{name}.dxf"
        cad.result("export.file", {"path": str(out), "options": {"patterns": "units"}})
        dxf = ezdxf.readfile(str(out))
        auditor = dxf.audit()
        assert not auditor.errors and not auditor.fixes, [e.message for e in auditor.errors + auditor.fixes]
        by_layer = polylines_by_layer(dxf)
        whole = by_layer["LOT-UNITS-WHOLE"]
        cut = by_layer.get("LOT-UNITS-CUT", [])
        assert (len(whole), len(cut)) == (row["whole_units"], row["cut_units"]), name
        # The unit type is written as extended data.
        types = {e.get_xdata("KADAKAS_UNIT")[0].value for _, e in whole[:50]}
        assert types == {hexagon["units"][0]["name"] if name == "hex" else "i_shape"}
    # Several unit types: an octagon with a square counts per type.
    octa = cad.result("unit_pattern.module_preset", {"preset": "octagon_square", "side": 100})["pattern"]
    cad.result("hardscape_style.update", {"id": "Pavers 200x100", "joints": {**octa, "spacing": 4}})
    row = cad.result("report.unit_pattern")["rows"][0]
    names = {u["name"]: u for u in row["by_unit"]}
    assert set(names) == {"octagon", "square"}
    assert sum(u["whole_units"] for u in names.values()) == row["whole_units"]
    out = tmp_path / "octa.dxf"
    cad.result("export.file", {"path": str(out), "options": {"patterns": "units"}})
    dxf = ezdxf.readfile(str(out))
    counted = {}
    for layer in ("LOT-UNITS-WHOLE", "LOT-UNITS-CUT"):
        for _, e in polylines_by_layer(dxf).get(layer, []):
            t = e.get_xdata("KADAKAS_UNIT")[0].value
            counted.setdefault(t, [0, 0])[0 if layer.endswith("WHOLE") else 1] += 1
    assert {k: tuple(v) for k, v in counted.items()} == {n: (u["whole_units"], u["cut_units"]) for n, u in names.items()}
    # to_objects tags the units with their type.
    made = cad.result("unit_pattern.to_objects", {"region": {"outline": rect(800, 800)}, "pattern": {**octa, "gap": 4}, "origin": [0, 0]})
    assert made["units"] > 10
    kinds = {o["data"].get("unit_type") for o in cad.result("objects.list", {"kind": "polyline"}) if "unit_type" in o["data"]}
    assert kinds == {"octagon", "square"}


def test_modules_from_shapes_and_from_a_dxf_block(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    a = add_poly(cad, rect(100, 100))
    b = add_poly(cad, [[100, 0], [200, 0], [200, 100], [100, 100]])
    m = cad.result("unit_pattern.module_from_shapes", {"ids": [a, b], "names": ["left", "right"]})
    assert m["pattern"]["a"] == [200.0, 0.0] and m["pattern"]["b"] == [0.0, 100.0] and "stack" in m["note"]
    r = cad.result("unit_pattern.layout", {"region": {"outline": rect(1000, 500)}, "pattern": m["pattern"], "origin": [0, 0], "include": ["units"], "limit": 0})
    assert r["summary"]["whole_units"] == 50
    assert {u["name"]: u["whole_units"] for u in r["summary"]["by_unit"]} == {"left": 25, "right": 25}
    # Shapes that overlap are refused with the reason.
    c = add_poly(cad, [[50, 0], [150, 0], [150, 100], [50, 100]])
    err = cad.error("unit_pattern.module_from_shapes", {"ids": [a, c], "a": [200, 0], "b": [0, 200]})
    assert "overlap" in err["message"]
    # A block of a DXF file: two shapes on two layers; arcs through bulges are kept.
    doc = ezdxf.new("R2010")
    doc.header["$INSUNITS"] = 4  # millimeters
    doc.layers.add("BIG")
    doc.layers.add("SMALL")
    blk = doc.blocks.new("HEXMOD")
    blk.add_lwpolyline([(0, 0), (100, 0), (100, 100), (0, 100)], close=True, dxfattribs={"layer": "BIG"})
    blk.add_lwpolyline([(100, 0), (150, 0), (150, 100), (100, 100)], close=True, dxfattribs={"layer": "SMALL"})
    doc.modelspace().add_blockref("HEXMOD", (0, 0))
    path = tmp_path / "module.dxf"
    doc.saveas(path)
    m = cad.result("unit_pattern.module_from_block", {"path": str(path), "block": "HEXMOD"})
    assert {u["name"] for u in m["pattern"]["units"]} == {"BIG", "SMALL"}
    assert m["pattern"]["a"] == [150.0, 0.0] and m["pattern"]["b"] == [0.0, 100.0]
    assert "no block" in cad.error("unit_pattern.module_from_block", {"path": str(path), "block": "NOPE"})["message"]
    # The path policy of import.file applies.
    assert cad.error("unit_pattern.module_from_block", {"path": "/etc/passwd", "block": "X"})["code"] != 0
    # A module in a hardscape style.
    cad.result("hardscape_style.create", {"name": "Mod", "joints": {**m["pattern"], "spacing": 3}})
    hid = cad.result("hardscape.add", {"boundary": rect(3000, 2000), "style": "Mod"})["id"]
    row = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
    assert row["bond"] == "module" and {u["name"] for u in row["by_unit"]} == {"BIG", "SMALL"}


def test_describe_and_mcp_list_the_pattern_methods(cad, mcp):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    assert {
        "unit_pattern.layout", "unit_pattern.to_objects", "unit_pattern.module_preset",
        "unit_pattern.module_from_shapes", "unit_pattern.module_from_block",
        "report.unit_pattern", "hardscape.from_shapes",
    } <= names
    assert "pattern.layout" not in names, "pattern.* is the hatch pattern family"
    assert "patterns" in d["io"]["export_options"]
    assert "workflow_parking_lot" in d["unit_patterns"] and "modules" in d["unit_patterns"]
    wf = d["unit_patterns"]["workflow_parking_lot"]
    assert all(step["method"] != "query.objects" for step in wf)
    assert all(step["method"] in names or step["method"] == "render.png" for step in wf)
    region = {"outline": [[0, 0], [2050, 0], [2050, 1050], [0, 1050]]}
    pattern = {"unit_length": 200, "unit_width": 100, "gap": 5, "bond": "stack"}
    r = mcp.method_json("hardscape_landscape", "unit_pattern.layout", {"region": region, "pattern": pattern, "origin": [0, 0], "include": ["units"]})
    assert r["summary"]["whole_units"] == 100
    r = mcp.method_json("reports", "report.unit_pattern", {})
    assert r["rows"] == [] and r["totals"]["complete"] is True
