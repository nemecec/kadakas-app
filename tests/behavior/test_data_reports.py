"""Scenario: a landscape architect defines records, attaches them to objects, builds a plant
list worksheet over the API, checks its totals, and exports it. Also the Data Manager."""

import base64
import csv
import io
import zipfile

import pytest
from pytest import approx

SQUARE = lambda side: [[0, 0], [side, 0], [side, side], [0, side]]  # noqa: E731


def make_style(cad, name, botanical, code, price, **fields):
    params = {"name": name, "botanical_name": botanical, "id_code": code, "category": "tree",
              "spread": 4000, "height": 8000, "price": price}
    params.update(fields)
    return cad.result("plant_style.create", params)["id"]


def place(cad, style, n, x0=0):
    return cad.result("plants.place", {"mode": "poly", "style": style, "points": [[x0 + i * 5000, 0] for i in range(n)]})["created"]


def add_hardscape(cad, material, side, **extra):
    shape = {"kind": "hardscape", "boundary": SQUARE(side), "material": material}
    shape.update(extra)
    return cad.execute({"op": "add_object", "shape": shape})[0]


@pytest.fixture
def planted(cad):
    """Three maples and two birches, priced 100 and 40."""
    maple = make_style(cad, "Field maple", "Acer campestre", "AC", 100, container="C10")
    birch = make_style(cad, "Silver birch", "Betula pendula", "BP", 40, category="tree")
    place(cad, maple, 3)
    place(cad, birch, 2, x0=40000)
    return maple, birch


# --- Record formats and records -------------------------------------------------------


def soil_fields():
    return [
        {"name": "Depth", "type": "dimension", "unit": "mm", "default": 300},
        {"name": "Grade", "type": "choice", "choices": ["A", "B", "C"]},
        {"name": "Wet", "type": "boolean"},
        {"name": "Samples", "type": "integer", "default": 1},
        {"name": "Note", "type": "text"},
        {"name": "Cover", "type": "area"},
        {"name": "Ratio", "type": "number", "default": 0.5},
    ]


def test_record_format_attach_set_detach_and_undo(cad):
    fid = cad.result("record_format.create", {"name": "Soil", "fields": soil_fields()})["id"]
    listing = cad.result("record_format.list")["formats"]
    assert [(f["id"], f["name"], f["used_by"]) for f in listing] == [(fid, "Soil", 0)]
    assert cad.result("record_format.get", {"id": "soil"})["fields"][1]["choices"] == ["A", "B", "C"]

    obj = add_hardscape(cad, "Granite", 2000)
    cad.result("record.attach", {"ids": [obj], "record": "Soil", "values": {"Depth": 450, "grade": "b"}})
    rec = cad.result("record.get", {"id": obj})["records"]["Soil"]
    assert rec == {"Depth": 450.0, "Grade": "B", "Wet": False, "Samples": 1, "Note": "", "Cover": 0.0, "Ratio": 0.5}
    # The record is stored in object.data under the record name.
    assert cad.result("objects.get", {"id": obj})["data"]["Soil"]["Depth"] == 450.0

    cad.result("record.set", {"ids": [obj], "record": fid, "values": {"Wet": True, "Samples": "4"}})
    rec = cad.result("record.get", {"id": obj})["records"]["Soil"]
    assert rec["Wet"] is True and rec["Samples"] == 4 and rec["Depth"] == 450.0
    assert cad.result("record_format.get", {"id": fid})["used_by"] == 1

    cad.result("record.detach", {"ids": [obj], "record": "Soil"})
    assert cad.result("record.get", {"id": obj})["records"] == {}
    # Each step undoes on its own.
    for _ in range(3):
        assert cad.result("undo")["done"]
    assert cad.result("record.get", {"id": obj})["records"] == {}
    assert cad.result("record_format.list")["formats"][0]["used_by"] == 0


def test_bad_records_are_refused_and_change_nothing(cad):
    cad.result("record_format.create", {"name": "Soil", "fields": soil_fields()})
    a = add_hardscape(cad, "Granite", 1000)
    b = add_hardscape(cad, "Brick", 1000)
    before = cad.doc()
    for values in ({"Depth": "deep"}, {"Grade": "Z"}, {"Samples": 1.5}, {"Nope": 1}, {"Wet": "maybe"}, {"Depth": 1e15}):
        assert cad.error("record.attach", {"ids": [a], "record": "Soil", "values": values})["code"] in (1, -32602)
    assert cad.error("record.attach", {"ids": [999], "record": "Soil"})["code"] == 1
    assert cad.error("record.attach", {"ids": [a], "record": "Missing"})["code"] == -32602
    assert cad.error("record.set", {"ids": [a], "record": "Soil", "values": {"Wet": True}})["code"] == 1  # not attached
    cad.result("record.attach", {"ids": [a], "record": "Soil"})
    assert "already" in cad.error("record.attach", {"ids": [b, a], "record": "Soil"})["message"]
    assert cad.result("record.get", {"id": b})["records"] == {}
    # set_data cannot bypass the checks.
    assert "record" in cad.error("execute", {"command": {"op": "set_data", "id": b, "set": {"Soil": {"Depth": "x"}}}})["message"]
    cad.result("undo")
    assert cad.doc() == before


def test_record_format_rules(cad):
    for bad in ({"name": "plant", "fields": []}, {"name": " ", "fields": []},
                {"name": "X", "fields": [{"name": "A", "type": "choice"}]},
                {"name": "X", "fields": [{"name": "A", "type": "text"}, {"name": "a", "type": "text"}]},
                {"name": "X", "fields": [{"name": "A", "type": "integer", "default": 1.5}]},
                {"name": "X", "fields": [{"name": "A", "type": "colour"}]}):
        assert cad.error("record_format.create", bad)["code"] in (1, -32602), bad
    fid = cad.result("record_format.create", {"name": "Soil", "fields": soil_fields()})["id"]
    assert cad.error("record_format.create", {"name": "SOIL", "fields": []})["code"] == 1
    obj = add_hardscape(cad, "Granite", 1000)
    cad.result("record.attach", {"ids": [obj], "record": fid, "values": {"Depth": 120}})
    assert "used by 1" in cad.error("record_format.delete", {"id": fid})["message"]

    # Renaming and editing the format updates attached records.
    fields = [f for f in soil_fields() if f["name"] != "Ratio"] + [{"name": "Source", "type": "text", "default": "survey"}]
    cad.result("record_format.update", {"id": fid, "name": "Ground", "fields": fields})
    assert cad.result("record.get", {"id": obj})["records"].keys() == {"Ground"}
    rec = cad.result("record.get", {"id": obj})["records"]["Ground"]
    assert rec["Depth"] == 120.0 and rec["Source"] == "survey" and "Ratio" not in rec
    cad.result("record.detach", {"ids": [obj], "record": "Ground"})
    cad.result("record_format.delete", {"id": "Ground"})
    assert cad.result("record_format.list")["formats"] == []


def test_plant_style_data_keeps_working_next_to_records(cad):
    cad.result("record_format.create", {"name": "Care", "fields": [{"name": "Water", "type": "choice", "choices": ["low", "high"]}]})
    sid = make_style(cad, "Field maple", "Acer campestre", "AC", 100)
    pid = place(cad, sid, 1)[0]
    cad.result("record.attach", {"ids": [pid], "record": "Care", "values": {"Water": "high"}})
    obj = cad.result("objects.get", {"id": pid})
    assert obj["data"]["plant"]["style"] == sid and obj["data"]["Care"]["Water"] == "high"
    cad.result("plant_style.update", {"id": sid, "spread": 6000})
    assert cad.result("objects.get", {"id": pid})["shape"]["spread"] == 6000
    assert cad.result("report.plant_schedule")["rows"][0]["count"] == 1


# --- Worksheets: the plant list ---------------------------------------------------------


def test_plant_list_template_totals(cad, planted):
    sid = cad.result("worksheet.create", {"name": "Plant list", "template": "plant_list"})["id"]
    r = cad.result("worksheet.evaluate", {"id": sid})
    assert r["problems"] == [] and r["errors"] == 0
    header = r["text"][0]
    assert header[:3] == ["Style", "Code", "Botanical name"] and header[6] == "Count" and header[8] == "Cost"
    rows = {row[0]: row for row in r["values"][1:-1]}
    assert rows["Field maple"][6] == 3 and rows["Field maple"][8] == approx(300)
    assert rows["Silver birch"][6] == 2 and rows["Silver birch"][8] == approx(80)
    assert rows["Field maple"][1] == "AC" and rows["Silver birch"][2] == "Betula pendula"
    total = r["values"][-1]
    assert total[0] == "Total" and total[6] == 5 and total[8] == approx(380)
    # The worksheet agrees with the plant schedule report.
    schedule = cad.result("report.plant_schedule")
    assert total[6] == schedule["total_count"] and total[8] == approx(schedule["total_price"])
    assert r["blocks"] == [{"name": "Plant list", "first_row": 1, "rows": 4, "matched": 5}]


def test_worksheet_is_live_and_cells_read_database_results(cad, planted):
    maple, _ = planted
    sid = cad.result("worksheet.create", {"name": "Plants", "template": "plant_list"})["id"]
    # Total count sits in G4 (header row 1, two groups, total row 4).
    cad.result("worksheet.set_cells", {"id": sid, "cells": {"K1": "Plants", "K2": "=G4", "K3": "=I4 / G4", "K4": "=IF(K2 > 5, \"large\", \"small\")"}})
    cad.result("worksheet.set_cell", {"id": sid, "address": "K3", "input": "=I4 / G4", "decimals": 2, "unit": "per plant"})
    r = cad.result("worksheet.evaluate", {"id": sid})
    assert r["cells"]["K2"] == 5 and r["cells"]["K3"] == approx(76) and r["cells"]["K4"] == "small"
    assert r["text"][2][10] == "76.00 per plant"
    place(cad, maple, 4, x0=100000)
    r = cad.result("worksheet.evaluate", {"id": sid})
    assert r["cells"]["K2"] == 9 and r["cells"]["K4"] == "large"


def test_worksheet_follows_undo_of_plants(cad, planted):
    maple, _ = planted
    sid = cad.result("worksheet.create", {"name": "Plants", "template": "plant_list"})["id"]
    assert cad.result("worksheet.evaluate", {"id": sid})["values"][-1][6] == 5
    place(cad, maple, 4, x0=100000)
    assert cad.result("worksheet.evaluate", {"id": sid})["values"][-1][6] == 9
    assert cad.result("undo")["done"]
    assert cad.result("worksheet.evaluate", {"id": sid})["values"][-1][6] == 5


def test_formulas_cycles_and_errors_never_hang(cad):
    sid = cad.result("worksheet.create", {"name": "Calc", "cells": {
        "A1": "10", "A2": "20", "A3": "=SUM(A1:A2)", "B1": "=B2", "B2": "=B1", "C1": "=1/0",
        "C2": "=IFERROR(C1, -1)", "C3": "=ROUND(A3 / 7, 2)", "C4": "=CONCAT(\"n=\", A3)", "D1": "=A1+B1"}})["id"]
    r = cad.result("worksheet.evaluate", {"id": sid})
    cells = r["cells"]
    assert cells["A3"] == 30 and cells["C2"] == -1 and cells["C3"] == approx(4.29) and cells["C4"] == "n=30"
    assert cells["B1"] == {"error": "#CYCLE!"} and cells["B2"] == {"error": "#CYCLE!"}
    assert cells["D1"] == {"error": "#CYCLE!"}, "a cell that reads a cycle shows the error"
    assert cells["C1"] == {"error": "#DIV/0!"}
    assert any("circular" in p for p in r["problems"])
    # A formula that cannot be read is refused when it is typed.
    assert cad.error("worksheet.set_cell", {"id": sid, "address": "E1", "input": "=SUM(("})["code"] == -32602
    assert cad.error("worksheet.set_cell", {"id": sid, "address": "1A", "input": "x"})["code"] == -32602
    # A long chain is calculated without trouble.
    cells = {"A1": "1"}
    cells.update({f"A{i}": f"=A{i - 1}+1" for i in range(2, 2001)})
    chain = cad.result("worksheet.create", {"name": "Chain", "cells": cells})["id"]
    assert cad.result("worksheet.evaluate", {"id": chain})["cells"]["A2000"] == 2000


def test_database_rows_with_criteria_records_and_grouping(cad):
    cad.result("record_format.create", {"name": "Paving", "fields": [{"name": "Depth", "type": "dimension", "default": 100}, {"name": "Frost proof", "type": "boolean"}]})
    a = add_hardscape(cad, "Granite", 2000)
    b = add_hardscape(cad, "Brick", 1000, thickness=60)
    c = add_hardscape(cad, "Granite", 3000)
    cad.execute({"op": "add_object", "shape": {"kind": "landscape_area", "boundary": SQUARE(5000), "name": "Lawn"}})
    cad.result("record.attach", {"ids": [a, c], "record": "Paving", "values": {"Depth": 150}})
    cad.result("record.attach", {"ids": [b], "record": "Paving", "values": {"Depth": 60, "Frost proof": True}})

    columns = [{"header": "Material", "expr": "Material"}, {"header": "Count", "expr": "Count", "summary": "sum"},
               {"header": "Area", "expr": "Area", "summary": "sum", "decimals": 1, "unit": "m2"},
               {"header": "Depth", "expr": "Paving.Depth", "summary": "max"}]

    count = iter(range(1000))

    def run(criteria, **extra):
        db = {"criteria": criteria, "columns": columns, "group_by": 0, "group_mode": "summary"}
        db.update(extra)
        sid = cad.result("worksheet.create", {"name": f"Sheet {next(count)}", "databases": [db]})["id"]
        return cad.result("worksheet.evaluate", {"id": sid})

    r = run('Kind = "hardscape"')
    assert [row[0] for row in r["values"]] == ["Material", "Brick", "Granite", "Total"]
    assert [row[1] for row in r["values"]] == ["Count", 1, 2, 3]
    assert [row[2] for row in r["values"]][1:] == [approx(1), approx(13), approx(14)]
    assert [row[3] for row in r["values"]][1:] == [60, 150, 150]
    assert r["text"][2][2] == "13.0 m2"

    # Field comparisons, records, AND/OR/NOT.
    assert run('Kind = "hardscape" AND Paving.Depth >= 100')["blocks"][0]["matched"] == 2
    assert run('HASRECORD("Paving") AND NOT Material = "Granite"')["blocks"][0]["matched"] == 1
    assert run("Paving.'Frost proof' = TRUE OR Area > 20")["blocks"][0]["matched"] == 2
    assert run('Layer = "Layer 1" AND Area > 3 AND Area < 20')["blocks"][0]["matched"] == 2
    assert run('Style = "Lawn"')["blocks"][0]["matched"] == 1
    assert run("Thickness < 100")["blocks"][0]["matched"] == 1

    # Detail mode lists every object with a subtotal per group, sorted descending by area.
    r = run('Kind = "hardscape"', group_mode="detail", sort=[{"column": 2, "descending": True}])
    assert [row[0] for row in r["values"]] == ["Material", "Brick", "Brick total", "Granite", "Granite", "Granite total", "Total"]
    assert [row[2] for row in r["values"]][3:6] == [approx(9), approx(4), approx(13)]

    # A criteria with an unknown name reports a problem and matches nothing.
    r = run("Bogus > 1")
    assert r["blocks"][0]["matched"] == 0 and any("criteria" in p for p in r["problems"])
    assert cad.error("worksheet.create", {"name": "Bad", "databases": [{"criteria": "Kind =", "columns": columns}]})["code"] == -32602


def test_set_database_and_templates(cad, planted):
    assert [t["id"] for t in cad.result("worksheet.templates")["templates"]] == ["plant_list", "hardscape_quantities", "area_schedule", "tree_survey"]
    sid = cad.result("worksheet.create", {"name": "Mine"})["id"]
    db = {"name": "Trees", "anchor": "B2", "criteria": 'Kind = "plant"', "columns": [{"header": "Species", "expr": "Species"}, {"header": "Count", "expr": "Count", "summary": "sum"}],
          "group_by": 0, "group_mode": "summary"}
    assert cad.result("worksheet.set_database", {"id": sid, "database": db})["index"] == 0
    r = cad.result("worksheet.evaluate", {"id": sid})
    assert r["cells"]["B2"] == "Species" and r["cells"]["B3"] == "Acer campestre" and r["cells"]["C3"] == 3
    assert r["cells"]["C5"] == 5
    db["criteria"] = 'Kind = "plant" AND Species = "Betula pendula"'
    cad.result("worksheet.set_database", {"id": sid, "index": 0, "database": db})
    assert cad.result("worksheet.evaluate", {"id": sid})["cells"]["C4"] == 2
    assert cad.error("worksheet.set_database", {"id": sid, "index": 4, "database": db})["code"] == -32602
    cad.result("worksheet.remove_database", {"id": sid, "index": 0})
    assert cad.result("worksheet.evaluate", {"id": sid})["cells"] == {}
    assert cad.error("worksheet.create", {"name": "X", "template": "nope"})["code"] == -32602
    cad.result("worksheet.rename", {"id": sid, "name": "Renamed"})
    assert [w["name"] for w in cad.result("worksheet.list")["worksheets"]] == ["Renamed"]
    assert cad.result("worksheet.get", {"id": "renamed"})["definition"]["databases"] == []
    cad.result("worksheet.delete", {"id": "Renamed"})
    assert cad.result("worksheet.list")["worksheets"] == []


def test_other_report_templates(cad):
    cad.execute({"op": "add_object", "shape": {"kind": "existing_tree", "at": [0, 0], "species": "Oak", "dbh": 600, "spread": 9000, "height": 18000, "condition": "good"}})
    cad.execute({"op": "add_object", "shape": {"kind": "existing_tree", "at": [9000, 0], "species": "Ash", "dbh": 300, "spread": 5000, "height": 9000, "condition": "poor", "action": "remove"}})
    add_hardscape(cad, "Granite", 2000, thickness=100)
    add_hardscape(cad, "Granite", 1000, thickness=100)
    cad.execute({"op": "add_object", "shape": {"kind": "landscape_area", "boundary": SQUARE(3000), "name": "Lawn"}})

    trees = cad.result("worksheet.evaluate", {"id": cad.result("worksheet.create", {"name": "Trees", "template": "tree_survey"})["id"]})
    assert trees["text"][1][1:] == ["Oak", "600 mm", "9.0 m", "18.0 m", "good", "retain", "7.2 m"]
    assert trees["text"][2][1:] == ["Ash", "300 mm", "5.0 m", "9.0 m", "poor", "remove", "3.6 m"]

    hard = cad.result("worksheet.evaluate", {"id": cad.result("worksheet.create", {"name": "Hard", "template": "hardscape_quantities"})["id"]})
    assert hard["text"][1] == ["Granite", "2", "5.00 m2", "100 mm", "0.50 m3"]
    assert hard["values"][-1][2] == approx(5)

    areas = cad.result("worksheet.evaluate", {"id": cad.result("worksheet.create", {"name": "Areas", "template": "area_schedule"})["id"]})
    first = [row[0] for row in areas["values"]]
    assert first == ["Kind", "hardscape", "hardscape", "hardscape total", "landscape_area", "landscape_area total", "Total"]
    assert areas["values"][-1][3] == approx(14)
    # The report.areas method gives the same total.
    assert sum(row["area_m2"] for row in cad.result("report.areas")["rows"]) == approx(14)


# --- Export ----------------------------------------------------------------------------


def test_export_csv_and_xlsx(cad, planted, tmp_path):
    sid = cad.result("worksheet.create", {"name": "Plant list: 2026", "template": "plant_list"})["id"]
    text = cad.result("worksheet.export", {"id": sid, "format": "csv"})["csv"]
    rows = list(csv.reader(io.StringIO(text)))
    assert rows[0][0] == "Style" and rows[-1][0] == "Total" and rows[-1][6] == "5" and rows[-1][8] == "380.00"
    assert {r[0] for r in rows[1:-1]} == {"Field maple", "Silver birch"}

    path = tmp_path / "plants.xlsx"
    out = cad.result("worksheet.export", {"id": sid, "format": "xlsx", "path": str(path)})
    assert out["bytes"] == path.stat().st_size
    with zipfile.ZipFile(path) as z:
        assert z.testzip() is None
        assert "xl/worksheets/sheet1.xml" in z.namelist()
        assert 'name="Plant list 2026"' in z.read("xl/workbook.xml").decode()
        sheet = z.read("xl/worksheets/sheet1.xml").decode()
    assert "Field maple" in sheet and "<v>380</v>" in sheet

    inline = cad.result("worksheet.export", {"id": sid, "format": "xlsx"})
    assert zipfile.ZipFile(io.BytesIO(base64.b64decode(inline["xlsx_base64"]))).testzip() is None

    csv_path = tmp_path / "plants.csv"
    cad.result("worksheet.export", {"id": sid, "format": "csv", "path": str(csv_path)})
    assert csv_path.read_text().startswith("Style,")
    assert "exists" in cad.error("worksheet.export", {"id": sid, "format": "csv", "path": str(csv_path)})["message"]
    cad.result("worksheet.export", {"id": sid, "format": "csv", "path": str(csv_path), "overwrite": True})
    assert cad.error("worksheet.export", {"id": sid, "format": "xlsx", "path": str(tmp_path / "wrong.csv")})["code"] == 1
    assert cad.error("worksheet.export", {"id": sid, "format": "pdf"})["code"] == -32602


def test_csv_export_defuses_formula_text(cad):
    sid = cad.result("worksheet.create", {"name": "Safe", "cells": {"A1": "'=1+1", "A2": "=1+1", "A3": "-5"}})["id"]
    text = cad.result("worksheet.export", {"id": sid, "format": "csv"})["csv"].split("\n")
    assert text[:3] == ["'=1+1", "2", "-5"]


def test_worksheets_survive_saving_and_loading(cad, planted, tmp_path):
    sid = cad.result("worksheet.create", {"name": "Plants", "template": "plant_list"})["id"]
    cad.result("record_format.create", {"name": "Soil", "fields": soil_fields()})
    cad.result("data_manager.create", {"name": "IFC", "template": "landscape"})
    before = cad.result("worksheet.evaluate", {"id": sid})["values"]
    doc = cad.result("document.get")
    cad.result("document.load", {"document": doc})
    assert cad.result("worksheet.evaluate", {"id": sid})["values"] == before
    assert len(cad.result("record_format.list")["formats"]) == 1
    assert len(cad.result("data_manager.list")["maps"]) == 1


# --- Data Manager ----------------------------------------------------------------------


def test_data_manager_maps_parameters_to_ifc_and_records(cad, planted):
    h = add_hardscape(cad, "Granite", 2000, thickness=120)
    cad.result("record_format.create", {"name": "Takeoff", "fields": [{"name": "Area", "type": "area"}, {"name": "Material", "type": "text"}]})
    cad.result("record.attach", {"ids": [h], "record": "Takeoff"})
    rules = [
        {"name": "area to record", "kinds": ["hardscape"], "source": {"type": "property", "name": "Area"}, "target": {"type": "record_field", "record": "Takeoff", "field": "Area"}},
        {"kinds": ["hardscape"], "source": {"type": "property", "name": "Material"}, "target": {"type": "record_field", "record": "Takeoff", "field": "Material"}},
        {"kinds": ["hardscape"], "source": {"type": "property", "name": "Area"}, "target": {"type": "ifc_property", "pset": "Cadektor_Quantities", "property": "Area", "ifc_type": "area"}},
        {"kinds": ["plant"], "source": {"type": "plant_field", "field": "price"}, "target": {"type": "ifc_property", "pset": "Cadektor_Planting", "property": "Price", "ifc_type": "real"}},
        {"source": {"type": "property", "name": "Layer"}, "target": {"type": "ifc_property", "pset": "Cadektor_Source", "property": "Layer", "ifc_type": "label"}},
        {"kinds": ["hardscape"], "source": {"type": "constant", "value": "paving"}, "target": {"type": "ifc_property", "pset": "Cadektor_Hardscape", "property": "Use", "ifc_type": "text"}},
    ]
    mid = cad.result("data_manager.create", {"name": "Map", "entities": [{"kind": "hardscape", "ifc_class": "IfcPavement"}, {"kind": "plant", "ifc_class": "IfcGeographicElement", "predefined_type": "VEGETATION"}], "rules": rules})["id"]
    assert cad.result("data_manager.check", {"id": mid})["problems"] == []

    ev = cad.result("data_manager.evaluate", {"id": "Map", "ids": [h]})
    obj = ev["objects"][0]
    assert obj["ifc_class"] == "IfcPavement"
    assert obj["properties"]["Cadektor_Quantities"]["Area"] == {"ifc_type": "area", "value": approx(4)}
    assert obj["properties"]["Cadektor_Hardscape"]["Use"]["value"] == "paving"
    assert obj["record_values"]["Takeoff"] == {"Area": approx(4), "Material": "Granite"}
    assert obj["problems"] == []

    plant = cad.result("objects.list", {"kind": "plant"})[0]["id"]
    p = cad.result("data_manager.evaluate", {"id": mid, "ids": [plant]})["objects"][0]
    assert p["ifc_class"] == "IfcGeographicElement" and p["predefined_type"] == "VEGETATION"
    assert p["properties"]["Cadektor_Planting"]["Price"]["value"] in (100.0, 40.0)
    assert "Cadektor_Quantities" not in p["properties"]

    # Apply writes the record targets, once, as one undo step.
    assert cad.result("data_manager.apply", {"id": mid}) == {"changed": 1, "problems": []}
    assert cad.result("record.get", {"id": h})["records"]["Takeoff"] == {"Area": approx(4), "Material": "Granite"}
    assert cad.result("data_manager.apply", {"id": mid})["changed"] == 0
    assert cad.result("undo")["done"]
    assert cad.result("record.get", {"id": h})["records"]["Takeoff"]["Material"] == ""

    # The map follows the document: a rule that points to a missing record is reported.
    cad.result("data_manager.update", {"id": mid, "rules": rules + [{"source": {"type": "property", "name": "Id"}, "target": {"type": "record_field", "record": "Nope", "field": "X"}}]})
    assert any("Nope" in p for p in cad.result("data_manager.check", {"id": mid})["problems"])
    assert cad.error("data_manager.create", {"name": "Bad", "rules": [{"source": {"type": "property", "name": ""}, "target": {"type": "record_field", "record": "R", "field": "F"}}]})["code"] == 1
    assert cad.error("data_manager.create", {"name": "Bad2", "template": "nope"})["code"] == -32602
    names = cad.result("data_manager.properties")
    assert "Area" in names["builtin"] and "price" in names["plant_fields"] and names["records"][0]["record"] == "Takeoff"
    cad.result("data_manager.delete", {"id": mid})
    assert cad.result("data_manager.list")["maps"] == []


def test_describe_has_examples_for_the_new_methods(cad):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    for method in ("record_format.create", "record.attach", "worksheet.create", "worksheet.set_cell", "worksheet.evaluate", "worksheet.export",
                   "data_manager.create", "data_manager.evaluate"):
        assert method in names
    assert d["data"]["worksheet"]["examples"] and d["data"]["data_manager"]["examples"]
    assert {"plant_list", "tree_survey"} <= {t["id"] for t in d["data"]["worksheet"]["templates"]}
    assert any(f["name"] == "SUM" for f in d["data"]["worksheet"]["functions"])
