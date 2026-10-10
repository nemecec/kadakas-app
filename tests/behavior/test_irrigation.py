"""Scenario: an irrigation designer builds a small zone over the API, checks the flow sums and the
pressure at the critical head against a hand calculation, lays out heads, and reads the reports.

The hand calculation uses Hazen-Williams in SI units:
    hf [m] = 10.67 * L * Q^1.852 / (C^1.852 * d^4.87)    (Q in m3/s, d in m, L in m)
    loss [kPa] = hf * 9.80665
and a valve loss of 100 * (Q [m3/h] / kv)^2 kPa.
"""

import csv
import io

import pytest
from pytest import approx

SQUARE = [[0, 0], [10000, 0], [10000, 6000], [0, 6000]]


def pipe_loss(lpm, d_mm, c, length_m):
    q, d = lpm / 60000.0, d_mm / 1000.0
    return 9.80665 * 10.67 * length_m * q**1.852 / (c**1.852 * d**4.87)


def valve_loss(lpm, kv):
    return 100.0 * (lpm * 60 / 1000 / kv) ** 2


def build_zone(cad):
    """POC 400 kPa / 100 l/min, 20 m PVC 50 mainline, valve zone 1 (kv 6), 30 m PE 32 lateral,
    10 m PE 25 branch, four Spray-4.0 heads (8 l/min each)."""
    cad.result("irrigation.poc.add", {"at": [0, 0], "static_pressure": 400, "available_flow": 100, "name": "Meter"})
    cad.result("irrigation.line.add", {"points": [[0, 0], [20000, 0]], "line_type": "mainline", "size": "50", "material": "PVC"})
    cad.result("irrigation.valve.add", {"at": [20000, 0], "zone": 1, "kv": 6, "name": "Beds"})
    cad.result("irrigation.line.add", {"points": [[20000, 0], [50000, 0]], "size": "32", "material": "PE"})
    cad.result("irrigation.line.add", {"points": [[50000, 0], [50000, 10000]], "size": "25", "material": "PE"})
    ids = []
    for at in ([50000, 0], [50000, 0], [50000, 10000], [50010, 10010]):
        ids.append(cad.result("irrigation.outlet.add", {"at": at, "model": "Spray-4.0"})["id"])
    return ids


def test_zone_flow_and_critical_pressure_match_the_hand_calculation(cad):
    ids = build_zone(cad)
    result = cad.result("irrigation.calculate", {})
    assert result["ok"], result["warnings"]
    zone = result["zones"][0]
    assert zone["zone"] == 1
    assert zone["flow_lpm"] == approx(32.0)

    p_valve = 400 - pipe_loss(32, 45.2, 150, 20) - valve_loss(32, 6)
    p_end = p_valve - pipe_loss(32, 26.2, 140, 30)
    p_far = p_end - pipe_loss(16, 20.4, 140, 10)
    assert zone["critical"]["pressure_kpa"] == approx(p_far, abs=0.01)
    assert zone["critical"]["id"] == ids[2]
    by_id = {o["id"]: o for o in zone["outlets"]}
    assert by_id[ids[0]]["pressure_kpa"] == approx(p_end, abs=0.01)
    assert all(o["ok"] for o in zone["outlets"])
    assert [p["label"] for p in zone["critical_path"]][0] == "Meter"
    assert zone["critical_path"][0]["pressure_kpa"] == 400.0
    assert zone["max_velocity_ms"] < 1.5

    # The calculation stored the zone in the outlets.
    assert cad.result("objects.get", {"id": ids[0]})["shape"]["zone"] == 1


def test_low_pressure_velocity_and_supply_warnings(cad):
    ids = build_zone(cad)
    poc = cad.result("objects.list", {"kind": "irrigation_poc"})[0]["id"]
    # A weak supply: the far heads fall below their minimum of 140 kPa.
    cad.result("irrigation.set", {"id": poc, "static_pressure": 150, "available_flow": 20})
    result = cad.result("irrigation.calculate", {"assign_zones": False})
    kinds = {w["kind"] for w in result["warnings"]}
    assert {"low_pressure", "flow_exceeds_supply"} <= kinds
    assert not result["ok"]
    low = [w for w in result["warnings"] if w["kind"] == "low_pressure"]
    assert ids[2] in {w["object"] for w in low}
    assert result["zones"][0]["critical"]["ok"] is False

    # A thin lateral: 32 l/min in a 12 mm pipe is far above 1.5 m/s.
    cad.result("irrigation.catalog.set", {"pipes": [{"size": "10", "material": "PE", "inner_diameter": 8}]})
    line = cad.result("objects.list", {"kind": "irrigation_line"})[1]["id"]
    cad.result("irrigation.set", {"id": line, "size": "10"})
    result = cad.result("irrigation.calculate", {"assign_zones": False})
    assert "high_velocity" in {w["kind"] for w in result["warnings"]}


def test_unconnected_outlets_and_catalog_gaps_warn(cad):
    build_zone(cad)
    lonely = cad.result("irrigation.outlet.add", {"at": [90000, 90000], "model": "Rotor-8"})["id"]
    result = cad.result("irrigation.calculate", {})
    unconnected = [w for w in result["warnings"] if w["kind"] == "unconnected"]
    assert [w["object"] for w in unconnected] == [lonely]
    # The lonely head has no zone.
    assert "zone" not in cad.result("objects.get", {"id": lonely})["shape"]
    sched = cad.result("irrigation.report.zones", {})
    assert sched["outlets_without_zone"] == 1
    # Bad parts are refused at the door.
    err = cad.error("irrigation.outlet.add", {"at": [0, 0], "model": "Ghost"})
    assert "unknown head model" in err["message"]
    err = cad.error("irrigation.line.add", {"points": [[0, 0], [1, 1]], "size": "32", "material": "Gold"})
    assert "not in the catalog" in err["message"]


def test_heads_connect_in_the_middle_of_a_lateral(cad):
    cad.result("irrigation.poc.add", {"at": [0, 0], "static_pressure": 400, "available_flow": 100})
    cad.result("irrigation.valve.add", {"at": [0, 0], "zone": 1})
    cad.result("irrigation.line.add", {"points": [[0, 0], [20000, 0]], "size": "32", "material": "PE"})
    for k in range(1, 11):
        cad.result("irrigation.outlet.add", {"at": [k * 2000, 20], "model": "Spray-2.5"})
    result = cad.result("irrigation.calculate", {})
    assert not [w for w in result["warnings"] if w["kind"] == "unconnected"]
    assert result["zones"][0]["flow_lpm"] == approx(40.0)


def test_connection_comes_from_geometry_within_the_tolerance(cad):
    cad.result("irrigation.poc.add", {"at": [0, 0], "static_pressure": 300, "available_flow": 80})
    cad.result("irrigation.valve.add", {"at": [10000, 0], "zone": 4, "kv": 5})
    cad.result("irrigation.line.add", {"points": [[0, 0], [10030, 30]], "line_type": "mainline", "size": "50", "material": "PVC"})
    cad.result("irrigation.line.add", {"points": [[9970, -30], [15000, 0]], "size": "32", "material": "PE"})
    cad.result("irrigation.outlet.add", {"at": [15000, 0], "model": "Rotor-8", "arc": 180})
    assert cad.result("irrigation.calculate", {})["zones"][0]["zone"] == 4
    # With a small tolerance the ends no longer meet the valve.
    result = cad.result("irrigation.calculate", {"tolerance": 10, "assign_zones": False})
    assert result["zones"] == []
    assert any(w["kind"] == "unconnected" for w in result["warnings"])


def test_reports_zone_schedule_pipe_schedule_pressure_and_materials(cad):
    build_zone(cad)
    # 4 heads, 32 l/min over a given area of 160 m2: 12 mm/h; 6 mm needs 30 minutes.
    sched = cad.result("irrigation.report.zones", {"target_depth": 6, "areas": {"1": 160}})
    row = sched["rows"][0]
    assert row["head_count"] == 4 and row["heads"] == [{"model": "Spray-4.0", "count": 4, "flow_lpm": approx(32.0)}]
    assert row["precipitation_mm_h"] == approx(12.0)
    assert row["run_time_min"] == approx(30.0)
    assert row["valves"] == ["Beds"]
    # Without a given area the area comes from the spacing of the heads.
    row = cad.result("irrigation.report.zones", {})["rows"][0]
    assert row["area_source"] == "layout"
    assert row["precipitation_mm_h"] == approx(60 * 32 / row["area_m2"])

    pipes = cad.result("irrigation.report.pipes", {})
    assert pipes["total_length_m"] == approx(60.0)
    rows = {(r["material"], r["size"]): r for r in pipes["rows"]}
    assert rows[("PVC", "50")]["length_m"] == approx(20.0)
    assert rows[("PE", "32")]["length_m"] == approx(30.0)
    assert pipes["total_cost"] == approx(20 * 2.6 + 30 * 1.4 + 10 * 0.9)

    pressure = cad.result("irrigation.report.pressure", {})["zones"][0]
    assert len(pressure["outlets"]) == 4

    materials = cad.result("irrigation.report.materials", {})
    heads = [r for r in materials["rows"] if r["category"] == "head"]
    assert heads[0]["quantity"] == 4 and heads[0]["total"] == approx(4 * 4.2)

    for method in ("zones", "pipes", "pressure", "materials"):
        text = cad.result(f"irrigation.report.{method}", {"format": "csv"})["csv"]
        assert len(list(csv.reader(io.StringIO(text)))) >= 2


def test_layout_helpers_fill_edge_and_drip(cad):
    # A square pattern in 10 m x 6 m at 2 m: 6 x 4 heads. One undo takes them all back.
    fill = cad.result("irrigation.layout.fill", {"boundary": SQUARE, "model": "Spray-4.0", "spacing": 2000, "arc": 90})
    assert fill["count"] == 24 and len(fill["ids"]) == 24
    assert len(cad.result("objects.list", {"kind": "irrigation_outlet"})) == 24
    assert cad.result("undo")["done"]
    assert cad.result("objects.list", {"kind": "irrigation_outlet"}) == []

    # A triangular pattern: rows 1.732 m apart, every second row moved by 1 m: 6 + 5 + 6 + 5 heads.
    tri = cad.result("irrigation.layout.fill", {"boundary": SQUARE, "model": "Spray-4.0", "spacing": 2000, "pattern": "triangular", "place": False})
    assert tri["count"] == 22 and tri["ids"] == []

    # Heads along an edge face to the left of the direction of travel (up).
    edge = cad.result("irrigation.layout.edge", {"path": [[0, 0], [10000, 0]], "model": "Spray-2.5", "spacing": 2500})
    assert edge["count"] == 5
    head = cad.result("objects.get", {"id": edge["ids"][0]})["shape"]
    assert head["arc"] == 180 and head["direction"] == approx(90.0)

    # Drip lines every 2 m: 3 lines of 10 m.
    drip = cad.result("irrigation.layout.drip", {"boundary": SQUARE, "spacing": 2000})
    assert drip["count"] == 3 and drip["total_length_m"] == approx(30.0)
    shape = cad.result("objects.get", {"id": drip["ids"][0]})["shape"]
    assert shape["line_type"] == "drip"


def test_catalog_csv_import_and_export(cad):
    csv_text = "name,radius,flow,pressure,kind,price\nMy spray,3000,5,200,spray,2.5\n"
    assert cad.result("irrigation.catalog.import_csv", {"kind": "heads", "text": csv_text})["imported"] == 1
    heads = {h["name"]: h for h in cad.result("irrigation.catalog.get")["heads"]}
    assert heads["My spray"]["radius"] == 3000 and heads["My spray"]["price"] == 2.5
    assert "Spray-4.0" in heads  # the built-in parts stay
    out = cad.result("irrigation.catalog.export_csv", {"kind": "heads", "document_only": True})["csv"]
    assert out.splitlines()[1].startswith("My spray,spray,3000")
    # The new head can be used and is calculated with its own flow.
    cad.result("irrigation.poc.add", {"at": [0, 0], "static_pressure": 300, "available_flow": 50})
    cad.result("irrigation.valve.add", {"at": [0, 0], "zone": 1})
    cad.result("irrigation.line.add", {"points": [[0, 0], [5000, 0]], "size": "25", "material": "PE"})
    cad.result("irrigation.outlet.add", {"at": [5000, 0], "model": "My spray", "arc": 180})
    zone = cad.result("irrigation.calculate", {})["zones"][0]
    assert zone["flow_lpm"] == approx(2.5)  # 5 l/min for a full circle, half for 180 degrees
    err = cad.error("irrigation.catalog.import_csv", {"kind": "heads", "text": "name,radius\nA,1\n"})
    assert "column" in err["message"]


def test_objects_save_load_move_and_undo(make_serve, tmp_path):
    first = make_serve()
    ids = build_zone(first)
    cad = first
    cad.result("irrigation.calculate", {})
    cad.result("irrigation.catalog.set", {"heads": [{"name": "Saved head", "radius": 2000, "flow": 3, "pressure": 200}]})
    path = tmp_path / "irrigation.json"
    cad.result("file.save", {"path": str(path)})
    cad = make_serve()
    assert cad.result("objects.list", {}) == []
    cad.result("file.open", {"path": str(path)})
    assert len(cad.result("objects.list", {"kind": "irrigation_outlet"})) == 4
    assert any(h["name"] == "Saved head" for h in cad.result("irrigation.catalog.get")["heads"])
    assert cad.result("irrigation.calculate", {})["zones"][0]["flow_lpm"] == approx(32.0)

    # Moving a head and turning it keep it a valid irrigation object.
    cad.result("execute", {"command": {"op": "move_objects", "ids": [ids[0]], "delta": [100, 0]}})
    assert cad.result("objects.get", {"id": ids[0]})["shape"]["at"][0] == 50100
    cad.result("irrigation.set", {"id": ids[0], "arc": 90, "direction": 45})
    assert cad.result("undo")["done"]
    assert cad.result("objects.get", {"id": ids[0]})["shape"]["arc"] == 360
    # A command with a bad shape is refused.
    err = cad.error("execute", {"command": {"op": "add_object", "shape": {"kind": "irrigation", "type": "outlet", "at": [0, 0], "model": "x", "arc": 0}}})
    assert "arc" in err["message"]


def test_svg_shows_coverage_and_describe_lists_irrigation(cad):
    build_zone(cad)
    svg = cad.result("render.svg")["svg"]
    assert "irrigation_outlet" in svg and "irrigation_line" in svg
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    assert {"irrigation.calculate", "irrigation.layout.fill", "irrigation.report.zones", "irrigation.catalog.set"} <= names
    assert len(d["irrigation"]["examples"]) >= 10
    # Worksheet templates list heads and pipes.
    ws = cad.result("worksheet.create", {"name": "Heads", "template": "irrigation_heads"})
    out = cad.result("worksheet.evaluate", {"id": ws["id"]})
    assert "Spray-4.0" in str(out)
