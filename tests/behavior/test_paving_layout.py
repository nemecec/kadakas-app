"""Scenario: an agent reads a parking lot from DXF, lays a paver pattern on it, checks the picture
and the counts, and writes a DXF for the contractor (feature spec PAT-01 to PAT-04, HRD-12, HRD-13).

The fixture `fixtures/parking_lot.dxf` has the lot outline and a planting island on layer LOT,
stall lines on STALLS, and a text. The coordinates are large map numbers.
"""

import base64

import pytest

from helpers import REPO

FIXTURE = REPO / "tests" / "behavior" / "fixtures" / "parking_lot.dxf"
ISLAND = [(22.0, 14.0), (30.0, 14.0), (31.0, 20.0), (26.0, 24.0), (21.0, 20.0)]


def shoelace(points):
    x0, y0 = points[0]
    pts = [(x - x0, y - y0) for x, y in points]
    return abs(sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(pts, pts[1:] + pts[:1]))) / 2


ISLAND_M2 = shoelace(ISLAND)
LOT_M2 = 60 * 40 - 12 * 12 / 2


def lay_the_lot(cad):
    cad.result("import.file", {"path": str(FIXTURE)})
    classes = {c["name"]: c["id"] for c in cad.result("document.summary")["classes"]}
    polys = cad.result("objects.list", {"kind": "polyline", "class": classes["LOT"]})
    assert len(polys) == 2 and all(p["shape"]["closed"] for p in polys)
    cad.result(
        "hardscape_style.create",
        {
            "name": "Pavers 200x100",
            "material": "Concrete pavers",
            "thickness": 80,
            "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5},
        },
    )
    made = cad.result(
        "hardscape.from_shapes",
        {"ids": [p["id"] for p in polys], "style": "Pavers 200x100", "holes_from_inner": True},
    )
    assert len(made["created"]) == 1 and made["created"][0]["holes"] == 1
    assert len(made["holes"]) == 1 and made["skipped"] == []
    hid = made["created"][0]["id"]
    cad.result("hardscape.set", {"id": hid, "align_to_edge": 0})
    return hid


def test_agent_lays_pavers_on_a_parking_lot_and_exports_the_units(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    hid = lay_the_lot(cad)
    # The sources are replaced, one hardscape is left, with the island as a hole.
    obj = cad.result("objects.get", {"id": hid})
    hs = obj["data"]["hardscape"]
    assert len(hs["holes"]) == 1 and hs["angle"] == 0.0 and hs["origin"] is not None
    png = base64.b64decode(cad.result("render.png", {"view": "top", "width": 320, "height": 240})["png_base64"])
    assert png[:8] == b"\x89PNG\r\n\x1a\n"

    rep = cad.result("report.pattern", {"waste_percent": 5})
    row = rep["rows"][0]
    assert row["id"] == hid
    assert row["region_area_m2"] == pytest.approx(LOT_M2 - ISLAND_M2, abs=0.01)
    assert row["whole_units"] > 90_000 and row["cut_units"] > 100
    assert row["order_units"] == row["whole_units"] + row["cut_units"]
    assert row["order_units_with_waste"] == -(-row["order_units"] * 105 // 100)
    assert row["area_laid_m2"] + row["gap_area_m2"] == pytest.approx(row["region_area_m2"], abs=1e-6)
    assert row["slivers"] <= row["cut_units"] and row["gap_length_m"] > 1000
    # report.hardscape carries the same block and the area without the island.
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
    by_layer = {}
    for e in dxf.modelspace():
        if e.dxftype() == "LWPOLYLINE":
            by_layer.setdefault(e.dxf.layer, []).append([(p[0], p[1]) for p in e.get_points()])
    whole = by_layer["LOT-UNITS-WHOLE"]
    cut = by_layer["LOT-UNITS-CUT"]
    assert (len(whole), len(cut)) == (row["whole_units"], row["cut_units"])
    assert len(by_layer["LOT"]) == 1, "the outline of the hardscape"
    # Real-world coordinates, and the island is empty.
    assert min(p[0] for u in whole for p in u) > 650_000_000 - 1
    area = sum(shoelace(u) for u in whole + cut) / 1e6
    assert area == pytest.approx(row["area_laid_m2"], abs=1e-3)
    island = [(650_000_000 + x * 1000, 6_470_000_000 + y * 1000) for x, y in ISLAND]
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


def test_pattern_layout_and_to_objects_on_the_lot(cad):
    hid = lay_the_lot(cad)
    r = cad.result("pattern.layout", {"id": hid, "include": ["units"], "limit": 5})
    assert r["summary"]["whole_units"] > 90_000 and len(r["units"]) == 5 and r["next_offset"] == 5
    # A small region of the same hardscape: units become ordinary objects, in one undo step.
    before = cad.result("document.summary")["object_count"]
    made = cad.result(
        "pattern.to_objects",
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
    cad.result("hardscape_style.create", {"name": "P", "joints": {"kind": "stack", "unit_length": 200, "unit_width": 100, "spacing": 5}})
    big = cad.result("hardscape.add", {"boundary": [[0, 0], [200000, 0], [200000, 150000], [0, 150000]], "style": "P"})["id"]
    err = cad.error("export.file", {"path": str(tmp_path / "x.dxf"), "options": {"patterns": "units"}})
    assert big in err["message"] and "limit" in err["message"]
    assert not (tmp_path / "x.dxf").exists()
    ok = cad.result("export.file", {"path": str(tmp_path / "ok.dxf"), "options": {"patterns": "none"}})
    assert ok["exported"] >= 1
    # The report names the same hardscape instead of failing.
    row = cad.result("report.pattern")["rows"][0]
    assert row["id"] == big and "limit" in row["error"]


def test_describe_and_mcp_list_the_pattern_methods(cad, mcp):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    assert {"pattern.layout", "pattern.to_objects", "report.pattern", "hardscape.from_shapes"} <= names
    assert "patterns" in d["io"]["export_options"]
    assert "workflow_parking_lot" in d["patterns"]
    # The agent tools reach them through their domains.
    region = {"outline": [[0, 0], [2050, 0], [2050, 1050], [0, 1050]]}
    pattern = {"unit_length": 200, "unit_width": 100, "gap": 5, "bond": "stack"}
    r = mcp.method_json("resources_symbols", "pattern.layout", {"region": region, "pattern": pattern, "origin": [0, 0], "include": ["units"]})
    assert r["summary"]["whole_units"] == 100
    r = mcp.method_json("reports", "report.pattern", {})
    assert r["rows"] == []
