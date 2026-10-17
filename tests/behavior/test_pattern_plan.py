"""Scenario: an agent reads a parking lot that is drawn as loose lines and arcs, makes regions of
it, divides the lot into zones, lays a soldier course along the kerb, checks the layout and
exports it for the contractor (feature spec PAT-06 to PAT-11).

The fixture `fixtures/parking_lot_loose.dxf` (made by `fixtures/make_parking_lot_loose.py`) has
lot A, a plain rectangle of 10 m by 6 m drawn as four lines with gaps of 0.3 to 0.4 mm; lot B with
a rounded corner (an ARC) and an island, all open by a fraction of a millimeter; stall lines; and
a rectangle that does not close.

The small case (lot A) has exact counts that are worked out by hand in `test_the_small_case`.
"""

import math

import pytest

from helpers import REPO

FIXTURE = REPO / "tests" / "behavior" / "fixtures" / "parking_lot_loose.dxf"
LOT_A_M2 = 60.0
LOT_B_M2 = 12.0 * 8.0 - 1.5 * 1.5 * (1 - math.pi / 4) - 2.0


def classes(cad):
    return {c["name"]: c["id"] for c in cad.result("document.summary")["classes"]}


def curves_of(cad, *names):
    cls = classes(cad)
    ids = []
    for n in names:
        ids += [o["id"] for o in cad.result("objects.list", {"class": cls[n]})]
    return ids


def style(cad, kind="running_bond"):
    return cad.result(
        "hardscape_style.create",
        {
            "name": "Pavers 200x100",
            "thickness": 80,
            "joints": {"kind": kind, "unit_length": 200, "unit_width": 100, "spacing": 5},
        },
    )["id"]


def test_loose_curves_become_regions_and_the_open_shape_is_reported(cad):
    cad.result("import.file", {"path": str(FIXTURE)})
    ids = curves_of(cad, "LOT_A", "LOT_B", "ISLAND")
    assert len(ids) == 4 + 5 + 4
    r = cad.result("region.from_curves", {"ids": ids, "tolerance": 1})
    assert r["complete"] is True
    # Lot A, lot B and the island of lot B. The overshoot of 30 mm is a stub, not a gap.
    depths = sorted((x["depth"], round(x["area_m2"], 3)) for x in r["regions"])
    assert [d for d, _ in depths] == [0, 0, 1]
    areas = {round(a, 1) for _, a in depths}
    assert {LOT_A_M2, 2.0} <= areas
    lot_b = next(x for x in r["regions"] if x["depth"] == 0 and len(x["holes"]) == 1)
    # Closing gaps of 0.2 to 0.5 mm moves the corners by half of that: a few thousand mm2 of 93 m2.
    assert lot_b["area_m2"] == pytest.approx(LOT_B_M2, abs=0.01)
    assert lot_b["role"] == "area"
    # The rounded corner is an arc, not a polygon of many corners.
    arcs = [v for v in lot_b["outline"] if isinstance(v, dict) and v.get("kind") == "point_on_arc"]
    assert len(arcs) == 1
    # Every gap that was closed is listed with its place and size: 4 + 5 + 4 corners, less the arc
    # (which meets two lines) and the corner that is exact.
    gaps = r["closed_gaps"]
    assert gaps["count"] == len(gaps["listed"]) >= 12
    assert all(0.0 < g["size_mm"] <= 1.0 for g in gaps["listed"])
    assert r["dead_ends"]["open"] == 0 and r["dead_ends"]["stubs"] == 1
    stub = r["dead_ends"]["listed"][0]
    assert stub["kind"] == "stub"
    assert stub["at"] == pytest.approx([-30.0, 6000.0], abs=0.5)
    assert stub["length_mm"] == pytest.approx(30.0, abs=0.5)

    # The shape that does not close is reported, not guessed: nothing is made of it.
    broken = curves_of(cad, "BROKEN")
    r = cad.result("region.from_curves", {"ids": broken, "tolerance": 1, "create": "hardscape"})
    assert r["regions"] == [] and r["created"] == [] and r["complete"] is False
    assert r["dead_ends"]["open"] == 2
    ends = sorted(tuple(round(c) for c in e["at"]) for e in r["dead_ends"]["listed"])
    assert ends == [(40_000, 3_970), (40_000, 4_000)]
    near = r["dead_ends"]["listed"][0]["nearest"]
    assert near["distance_mm"] == pytest.approx(30.0)
    # With a tolerance over the gap the gap closes, and says how big it was.
    r = cad.result("region.from_curves", {"ids": broken, "tolerance": 31})
    assert r["complete"] is True and len(r["regions"]) == 1
    assert r["closed_gaps"]["listed"][0]["size_mm"] == pytest.approx(30.0)


def test_regions_become_hardscapes_with_the_island_as_a_hole(cad):
    cad.result("import.file", {"path": str(FIXTURE)})
    ids = curves_of(cad, "LOT_A", "LOT_B", "ISLAND")
    st = style(cad)
    r = cad.result(
        "region.from_curves", {"ids": ids, "tolerance": 1, "create": "hardscape", "style": st}
    )
    made = r["created"]
    assert len(made) == 2 and all(m["kind"] == "hardscape" for m in made)
    rows = {row["id"]: row for row in cad.result("report.hardscape")["rows"]}
    areas = sorted(rows[m["id"]]["plan_area_m2"] for m in made)
    assert areas[0] == pytest.approx(LOT_A_M2, abs=0.01) and areas[1] == pytest.approx(LOT_B_M2, abs=0.01)
    assert sorted(rows[m["id"]]["holes"] for m in made) == [0, 1]
    # One undo takes both away.
    cad.result("undo")
    assert cad.result("report.hardscape")["rows"] == []
    # Landscape areas and plain polygons work too, and dry_run makes nothing.
    r = cad.result("region.from_curves", {"ids": ids, "create": "polyline", "dry_run": True})
    assert len(r["created"]) == 3 and r["dry_run"] is True
    assert cad.result("objects.list", {"kind": "polyline"}) == []
    r = cad.result("region.from_curves", {"ids": ids, "create": "landscape_area", "name": "Lawn"})
    assert len(r["created"]) == 2


def lot_a(cad):
    """Lot A as a hardscape with the pavers, the pattern starting at the corner."""
    cad.result("import.file", {"path": str(FIXTURE)})
    ids = curves_of(cad, "LOT_A")
    st = style(cad)
    r = cad.result("region.from_curves", {"ids": ids, "tolerance": 1, "create": "hardscape", "style": st})
    hid = r["created"][0]["id"]
    return hid


def test_the_small_case_has_exact_counts(cad, tmp_path):
    """Lot A: 10,000 by 6,000 mm, pavers of 200 by 100 mm with 5 mm joints in a running bond.

    A soldier course along the bottom edge (stones on end: 100 mm along the kerb, 200 mm deep):
    steps of 105 mm along 10,000 mm are 95 whole units (95 x 105 = 9,975) and a cut unit of 25 mm.
    The course takes the strip y = 0 to 205 (its depth and one joint) from the field. The field
    starts at y = 205 and at x = 0 (align_field), so its rows are 100 mm units at a step of 105 mm
    from y = 205: 55 whole rows (the last one ends at y = 5,975), and a row cut at y = 6,000 that is
    20 mm tall. Along x a step of 205 mm: even rows have 48 whole units and a cut one at the end (the
    unit at 9,840 is cut to 160 mm); odd rows start with a half unit (97.5 mm) and have 48 whole
    units and a cut one (57.5 mm). 28 even rows and 27 odd rows give 55 x 48 = 2,640 whole units and
    28 + 2 x 27 = 82 cut units; the last row of 20 mm has 1 + 48 + 1 = 50 cut units. The field is
    2,640 whole and 132 cut. With the course: 2,735 whole and 133 cut.

    A zone line at x = 6,000 cuts the one unit of each row that crosses it into two: 55 whole units
    become cut, each giving 2 cut units, and the last row has one more: 2,680 whole and 244 cut."""
    hid = lot_a(cad)
    r = cad.result(
        "hardscape.course",
        {"id": hid, "courses": [{"name": "Kerb", "path": [[0, 0], [10_000, 0]], "side": "left",
                                  "corner": "miter", "align_field": True}]},
    )
    kerb = r["courses"][0]
    assert kerb["units"] == 96 and kerb["corners"] == 0 and kerb["length_m"] == pytest.approx(10.0)
    assert kerb["joints"]["narrowest_mm"] == kerb["joints"]["widest_mm"] == 5.0
    rep = cad.result("report.unit_pattern", {"ids": [hid], "waste_percent": 5})
    row = rep["rows"][0]
    assert (row["whole_units"], row["cut_units"]) == (2735, 133)
    by = {g["name"]: g for g in row["groups"]}
    assert (by["Kerb"]["whole_units"], by["Kerb"]["cut_units"]) == (95, 1)
    assert (by["field"]["whole_units"], by["field"]["cut_units"]) == (2640, 132)
    assert len(row["by_unit"]) == 1 and row["by_unit"][0]["name"] == "200x100"

    # The zone: a drawn line, followed.
    line = cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [6000, -50], "b": [6000, 6050]}})[0]
    z = cad.result("hardscape.split", {"id": hid, "lines": [line]})
    assert [zn["area_m2"] for zn in z["zones"]] == pytest.approx([6.0 * 5.795, 4.0 * 5.795])
    assert z["zones_area_m2"] == pytest.approx(10.0 * 5.795)
    z = cad.result(
        "hardscape.split",
        {"id": hid, "zones": [{"index": 0, "name": "Stalls"}, {"index": 1, "name": "Aisle"}]},
    )
    assert [zn["name"] for zn in z["zones"]] == ["Stalls", "Aisle"]
    rep = cad.result("report.unit_pattern", {"ids": [hid], "waste_percent": 5})
    row = rep["rows"][0]
    assert (row["whole_units"], row["cut_units"]) == (2680, 244)
    assert row["order_units"] == 2680 + 244
    assert row["order_units_with_waste"] == math.ceil(2924 * 1.05)
    assert [g["name"] for g in row["groups"]] == ["Stalls", "Aisle", "Kerb"]
    assert sum(g["whole_units"] for g in row["groups"]) == 2680
    assert sum(g["cut_units"] for g in row["groups"]) == 244
    area = sum(g["area_laid_m2"] for g in row["groups"])
    assert area == pytest.approx(row["area_laid_m2"])
    assert rep["totals"]["whole_units"] == 2680 and rep["totals"]["complete"] is True

    # The check finds one thing: the piece where the zone line cuts the last row of 20 mm. It is
    # 42.5 mm by 20 mm, 4.25 percent of a unit (under the 5 percent of a small piece).
    chk = cad.result("unit_pattern.check", {"ids": [hid]})
    obj = chk["objects"][0]
    assert obj["ok"] is False and len(obj["findings"]) == 1
    f = obj["findings"][0]
    assert f["kind"] == "small_piece" and f["group"] == "Aisle"
    assert f["at"] == pytest.approx([6021.25, 5990.0]) and f["size"] == pytest.approx(0.0425)
    assert chk["totals"]["counts"] == {
        "coverage": 0, "joint_too_narrow": 0, "joint_too_wide": 0, "outside_area": 0,
        "overlap": 0, "small_piece": 1, "wrong_size": 0, "zone_edge_joint": 0,
    }
    assert obj["units"] == 2680 + 244
    # With a smaller minimum share there is nothing at all.
    assert cad.result("unit_pattern.check", {"ids": [hid], "min_share": 0.04})["objects"][0]["ok"] is True

    # Moving the line moves the zones; the counts stay (every row is still cut once).
    cad.execute({"op": "move_objects", "ids": [line], "delta": [1000, 0]})
    plan = cad.result("hardscape.plan", {"id": hid})
    assert [zn["area_m2"] for zn in plan["zones"]] == pytest.approx([7.0 * 5.795, 3.0 * 5.795])
    assert plan["zones"][0]["name"] == "Stalls"
    row = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
    assert (row["whole_units"], row["cut_units"]) == (2680, 244)

    # Without the alignment of the field the joint to the course is wrong, and the check says where.
    cad.result("hardscape.set", {"id": hid, "origin": [0, 0], "angle": 0})
    chk = cad.result("unit_pattern.check", {"ids": [hid]})["objects"][0]
    assert chk["ok"] is False
    kinds = {f["kind"] for f in chk["findings"]}
    assert "joint_too_wide" in kinds or "small_piece" in kinds
    f = next(f for f in chk["findings"] if f["kind"] in ("joint_too_wide", "small_piece"))
    assert 0.0 <= f["at"][0] <= 10_000.0 and 0.0 <= f["at"][1] <= 400.0


def test_the_dxf_has_blocks_zones_and_the_counts_of_the_report(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    hid = lot_a(cad)
    cad.result(
        "hardscape.course",
        {"id": hid, "courses": [{"name": "Kerb", "path": [[0, 0], [10_000, 0]], "align_field": True}]},
    )
    cad.result("hardscape.split", {"id": hid, "lines": [[[6000, -50], [6000, 6050]]]})
    row = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
    assert (row["whole_units"], row["cut_units"]) == (2680, 244)
    sizes = {}
    for mode in ("units", "blocks"):
        out = tmp_path / f"lot-{mode}.dxf"
        r = cad.result("export.file", {"path": str(out), "format": "dxf", "options": {"patterns": mode}})
        assert r["path"] == str(out)
        sizes[mode] = out.stat().st_size
        dxf = ezdxf.readfile(out)
        audit = dxf.audit()
        assert not audit.errors, [e.message for e in audit.errors]
        whole = cut = 0
        layers = set()
        for e in dxf.modelspace():
            layer = e.dxf.layer
            if layer.endswith("-UNITS-WHOLE"):
                assert e.dxftype() == ("INSERT" if mode == "blocks" else "LWPOLYLINE")
                whole += 1
                layers.add(layer)
            elif layer.endswith("-UNITS-CUT"):
                assert e.dxftype() == "LWPOLYLINE"
                cut += 1
                layers.add(layer)
        assert (whole, cut) == (row["whole_units"], row["cut_units"]), mode
        # The field, the zone and the course have their own layers.
        assert any("ZONE-" in l for l in layers) and any("COURSE-KERB" in l for l in layers), layers
        if mode == "blocks":
            # One block holds the unit, and an INSERT puts it where the unit is.
            names = [b.name for b in dxf.blocks if b.name.startswith("KADAKAS_")]
            assert names == ["KADAKAS_200X100"], names
            block = dxf.blocks.get("KADAKAS_200X100")
            poly = [e for e in block if e.dxftype() == "LWPOLYLINE"][0]
            pts = [(p[0], p[1]) for p in poly.get_points()]
            xs = sorted({round(abs(p[0]), 6) for p in pts}), sorted({round(abs(p[1]), 6) for p in pts})
            assert {100.0, 50.0} == {xs[0][0], xs[1][0]}
            inserts = [e for e in dxf.modelspace() if e.dxftype() == "INSERT"]
            assert len(inserts) == row["whole_units"]
            # A unit of the course stands on end: its block is turned by 90 degrees.
            course_inserts = [e for e in inserts if "COURSE" in e.dxf.layer]
            assert len(course_inserts) == 95
    assert sizes["blocks"] < sizes["units"]


def test_the_island_and_the_arc_lot_with_stall_zones_conserve_area(cad):
    cad.result("import.file", {"path": str(FIXTURE)})
    st = style(cad)
    r = cad.result(
        "region.from_curves",
        {"ids": curves_of(cad, "LOT_B", "ISLAND"), "tolerance": 1, "create": "hardscape", "style": st},
    )
    hid = r["created"][0]["id"]
    stalls = curves_of(cad, "STALLS")
    z = cad.result("hardscape.split", {"id": hid, "lines": stalls})
    assert len(z["zones"]) == 4
    assert z["zones_area_m2"] == pytest.approx(z["area_m2"], abs=1e-9)
    # The island lies in the second stall zone as a hole of it: the zones add up to the lot less it.
    assert z["area_m2"] == pytest.approx(LOT_B_M2, abs=0.01)
    # The zones' counts add up to the totals, and the check finds no overlap between zones.
    cad.result("hardscape.set", {"id": hid, "origin": [20_000, 0], "angle": 0})
    row = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
    assert len(row["groups"]) == 4
    assert sum(g["whole_units"] for g in row["groups"]) == row["whole_units"]
    assert sum(g["cut_units"] for g in row["groups"]) == row["cut_units"]
    chk = cad.result("unit_pattern.check", {"ids": [hid]})["objects"][0]
    assert chk["counts"]["overlap"] == 0 and chk["counts"]["outside_area"] == 0
    # A zone with a pattern of its own, aligned to the line, and a name.
    z = cad.result(
        "hardscape.split",
        {"id": hid, "zones": [{"index": 1, "name": "Herringbone",
                                "pattern": {"bond": "herringbone", "unit_length": 200, "unit_width": 100, "gap": 5},
                                "angle": 45}]},
    )
    assert z["zones"][1]["name"] == "Herringbone"
    row = cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]
    herring = next(g for g in row["groups"] if g["name"] == "Herringbone")
    assert herring["bond"] == "herringbone"
    assert row["bond"] == "mixed"
    chk = cad.result("unit_pattern.check", {"ids": [hid]})["objects"][0]
    assert chk["counts"]["overlap"] == 0


def test_the_curved_course_reports_its_joints(cad):
    cad.result("import.file", {"path": str(FIXTURE)})
    hid = None
    st = style(cad)
    r = cad.result("region.from_curves", {"ids": curves_of(cad, "LOT_B"), "tolerance": 1, "create": "hardscape", "style": st})
    hid = r["created"][0]["id"]
    # A course round the whole outline, with the rounded corner: units stay straight on the arc.
    c = cad.result(
        "hardscape.course",
        {"id": hid, "courses": [{"name": "Edge", "along": "outline", "corner": "corner_unit", "align_field": False}]},
    )
    edge = c["courses"][0]
    assert edge["corners"] == 4 + 0 or edge["corners"] >= 3
    j = edge["joints"]
    assert j["narrowest_mm"] == pytest.approx(5.0, abs=1e-6)
    # On the arc of radius 1,500 mm the joint widens to 5 + 200 x 100 / 1,300 at most.
    assert 5.0 <= j["widest_mm"] <= 5.0 + 200 * 100 / 1300 * 1.1 + 1.0
    chk = cad.result("unit_pattern.check", {"ids": [hid], "max_joint_mm": 25})["objects"][0]
    assert chk["counts"]["overlap"] == 0, chk["findings"][:3]
    assert chk["counts"]["joint_too_narrow"] == 0, chk["findings"][:3]
    assert any(g["name"] == "Edge" for g in cad.result("report.unit_pattern", {"ids": [hid]})["rows"][0]["groups"])


def test_describe_and_mcp_list_the_new_methods(cad, mcp):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    new = {
        "region.from_curves", "hardscape.split", "hardscape.course", "hardscape.course_remove",
        "hardscape.plan", "hardscape.plan_clear", "unit_pattern.check",
    }
    assert new <= names
    up = d["unit_patterns"]
    assert {"regions_from_curves", "zones", "courses", "check"} <= set(up)
    wf = [step["method"] for step in up["workflow_parking_lot"]]
    for m in ("region.from_curves", "hardscape.split", "hardscape.course", "unit_pattern.check", "export.file"):
        assert m in wf
    assert all(m in names or m == "render.png" for m in wf)
    assert "blocks" in str(d["io"]["export_options"]["patterns"])
    # The MCP domain tool takes the new methods.
    r = mcp.method_json("hardscape_landscape", "unit_pattern.check", {})
    assert r["objects"] == [] and r["totals"]["complete"] is True
    result = mcp.method("hardscape_landscape", "region.from_curves", {"ids": []})
    assert result["isError"] is True


def test_an_island_that_touches_the_outline_is_checked_before_it_is_paved(cad):
    def line(a, b):
        return cad.execute({"op": "add_object", "shape": {"kind": "line", "a": a, "b": b}})[0]

    def box(x0, y0, x1, y1):
        return [line([x0, y0], [x1, y0]), line([x1, y0], [x1, y1]), line([x1, y1], [x0, y1]), line([x0, y1], [x0, y0])]

    ids = box(0, 0, 10_000, 6_000) + box(4_000, 2_000, 6_000, 3_000)
    ids.append(line([5_000, 3_000], [5_000, 6_000]))  # a stall line from the island kerb to the lot
    st = style(cad)
    r = cad.result("region.from_curves", {"ids": ids, "tolerance": 1, "create": "hardscape", "style": st, "dry_run": True})
    roles = [x["role"] for x in r["regions"]]
    assert "island?" in roles and any("may be an island" in w for w in r["warnings"])
    island = roles.index("island?")
    assert all(c["region"] != island for c in r["created"])
    # A doubtful region is only made when the caller lists it.
    r = cad.result("region.from_curves", {"ids": ids, "create": "polyline", "regions": [island]})
    assert [c["region"] for c in r["created"]] == [island]
