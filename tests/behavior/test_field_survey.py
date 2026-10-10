"""Scenario: a landscape architect imports a survey, checks it, builds the site and the plantings, and sets
display rules. All through the API."""

from pytest import approx

SURVEY = """P,N,E,Z,Code
1,6672000.0,385000.0,10.00,GROUND
2,6672010.0,385000.0,10.20,GROUND
3,6672000.0,385010.0,10.40,GROUND
4,6672010.0,385010.0,10.60,GROUND
5,6672005.0,385005.0,10.30,TREE
"""


def test_survey_points_become_loci_with_codes_and_a_georeference(cad, tmp_path):
    path = tmp_path / "points.csv"
    path.write_text(SURVEY)
    r = cad.result("survey.import_points", {"path": str(path), "crs": "EPSG:3067"})
    assert r["imported"] == 5 and r["skipped_count"] == 0
    assert r["georeference"]["crs"] == "EPSG:3067"
    loci = cad.result("objects.list", {"kind": "locus3d"})
    assert len(loci) == 5
    tree = next(o for o in loci if o["data"]["code"] == "TREE")
    assert tree["data"]["point_id"] == "5"
    # The center of the survey is the origin of the drawing, in millimeters.
    assert tree["shape"]["p"] == approx([0, 0, 10300])
    # A site model from the points keeps the heights.
    ids = [o["id"] for o in loci]
    site = cad.result("site.create", {"source_ids": ids})["id"]
    assert cad.result("site.elevation", {"id": site, "x": 0, "y": 0})["z"] == approx(10300)
    # One undo step takes the import back.
    cad.result("undo")
    cad.result("undo")
    assert cad.result("document.summary")["object_count"] == 0
    assert cad.result("document.summary")["georeference"] is None


def test_survey_columns_delimiters_and_units(cad):
    r = cad.result(
        "survey.import_points",
        {"text": "10 20 30 oak\n11 21 31 pine\n", "columns": ["E", "N", "Z", "C"], "delimiter": "space", "unit": "ft"},
    )
    assert r["imported"] == 2
    codes = sorted(o["data"]["code"] for o in cad.result("objects.list", {"kind": "locus3d"}))
    assert codes == ["oak", "pine"]
    assert cad.error("survey.import_points", {"text": "1,2,3,4,5", "columns": ["Q"]})["code"] == -32602
    assert cad.error("survey.import_points", {"text": "a,b,c"})["code"] == 3


def test_a_tree_survey_makes_existing_trees_and_the_report(cad):
    csv = "ID;E;N;Species;DBH (cm);Height (m);Spread (m);Condition;Action\nT1;5;7;Oak;50;15;9;good;retain\nT2;20;7;Birch;30;9;5;poor;remove\n"
    r = cad.result("tree.import_survey", {"text": csv})
    assert r["imported"] == 2
    rows = cad.result("report.existing_trees")["rows"]
    assert sorted(x["rpz_radius"] for x in rows) == [3600.0, 6000.0]
    assert {x["action"] for x in rows} == {"retain", "remove"}


def test_source_data_checks_report_errors_and_simplify_changes_the_site(cad):
    ids = []
    for x in range(6):
        for y in range(6):
            z = 6000 if (x, y) == (3, 3) else 100
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 1000, y * 1000, z]}})
    ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [1000, 1000, 900]}})
    site = cad.result("site.create", {"source_ids": ids})["id"]
    c = cad.result("site.check", {"id": site})
    assert c["clean"] is False
    assert c["counts"]["duplicate_xy"] == 1 and c["counts"]["spikes"] == 1
    kinds = {i["kind"] for i in c["issues"]}
    assert kinds == {"duplicate_xy", "spike"}
    before = len(cad.result("objects.get", {"id": site})["shape"]["points"])
    r = cad.result("site.simplify", {"id": site, "tolerance": 50})
    assert r["removed_points"] > 0
    assert len(cad.result("objects.get", {"id": site})["shape"]["points"]) < before
    assert cad.result("undo")["done"]
    assert len(cad.result("objects.get", {"id": site})["shape"]["points"]) == before
    assert cad.error("site.simplify", {"id": site, "tolerance": 0})["code"] == -32602


def test_the_survey_point_sets_the_georeference_and_the_grid_shows(cad):
    cad.result("georef.survey_point", {"model": [0, 0], "map": [385000, 6672000], "crs": "EPSG:3067"})
    cad.execute({"op": "add_object", "shape": {"kind": "rect", "min": [-2000, -2000], "max": [25000, 15000]}})
    assert cad.result("grid.set", {"spacing": 10})["georeferenced"] is True
    svg = cad.result("render.svg")
    svg = svg["svg"] if isinstance(svg, dict) else svg
    assert 'data-kind="ne_grid"' in svg and "E 385010" in svg and "N 6672010" in svg
    cad.result("grid.set", {"spacing": None})
    svg = cad.result("render.svg")
    svg = svg["svg"] if isinstance(svg, dict) else svg
    assert "ne_grid" not in svg


def test_phases_hide_and_color_and_a_visualization_is_only_a_display_rule(cad):
    ids = []
    for x, cond in [(0, "good"), (5000, "poor"), (10000, "poor")]:
        ids += cad.execute({"op": "add_object", "shape": {"kind": "existing_tree", "at": [x, 0], "dbh": 300, "spread": 4000, "height": 8000, "condition": cond}})
    cad.result("phase.set", {"ids": [ids[2]], "phase": "demolished"})
    assert cad.result("phase.report")["totals"] == {"existing": 0, "new": 2, "demolished": 1}
    before = [cad.result("objects.get", {"id": i}) for i in ids]
    cad.result("phase.display", {"demolished": {"visible": False}})
    svg = cad.result("render.svg")
    svg = svg["svg"] if isinstance(svg, dict) else svg
    assert f'data-id="{ids[0]}"' in svg and f'data-id="{ids[2]}"' not in svg
    v = cad.result("visualization.auto", {"field": "Condition", "target": "both"})
    assert [r["count"] for r in v["legend"]] == [1, 1] or sorted(r["count"] for r in v["legend"]) == [1, 2]
    after = [cad.result("objects.get", {"id": i}) for i in ids]
    # The visualization changed no object and no data.
    assert after == before
    cad.result("visualization.clear")
    assert cad.result("view_rules.get").get("visualization") is None


def test_plant_care_fields_tags_hydrozones_and_terrain(cad):
    style = cad.result(
        "plant_style.create",
        {"name": "Lavender", "id_code": "LA", "common_name": "Lavender", "botanical_name": "Lavandula", "size": "P9", "container": "C2", "spread": 600, "height": 500,
         "irrigation": {"water_need": "low", "hydrozone": "Z1"}, "maintenance": {"hours_per_year": 0.4, "interval": "yearly"}},
    )["id"]
    ids = []
    for x in range(5):
        for y in range(5):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 5000, y * 5000, 100 + x * 50]}})
    cad.result("site.create", {"source_ids": ids})
    r = cad.result("plants.place", {"mode": "hedgerow", "style": style, "path": [[0, 2000], [10000, 2000]], "spacing": 1000, "rows": 2, "row_spacing": 600, "tag": True, "on_terrain": True})
    assert r["count"] == 21 and r["tag"]
    plant = cad.result("objects.get", {"id": r["created"][0]})
    assert plant["data"]["ground_z"] == approx(100, abs=1)
    cad.result("plant_tag.set", {"ids": [r["tag"]], "levels": 3, "bubble": True})
    svg = cad.result("render.svg")
    svg = svg["svg"] if isinstance(svg, dict) else svg
    assert "plant_tag_bubble" in svg and "P9, C2" in svg
    rep = cad.result("irrigation.report.hydrozones", {"eto": 1000})
    assert rep["rows"][0]["hydrozone"] == "Z1" and rep["rows"][0]["plants"] == 21
    assert rep["rows"][0]["annual_water_l"] > 0
    csv = cad.result("plant_style.export_csv")["csv"]
    assert "water_need" in csv and "low,Z1" in csv


def test_curved_grades_grade_modifiers_and_pads_with_retaining_edges(cad):
    ids = []
    for x in range(5):
        for y in range(5):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 5000, y * 5000, 0]}})
    site = cad.result("site.create", {"source_ids": ids})["id"]
    path = [[2000, 6000], {"p": [3171.573, 3171.573], "kind": "point_on_arc"}, {"p": [6000, 2000], "kind": "point_on_arc"}]
    g = cad.result("grade.add", {"path": path, "elevation_a": 5000, "slope": {"percent": 2}})
    assert g["grade"]["percent"] == approx(2.0, abs=0.01)
    cad.result("grade.add", {"a": [1000, 15000], "b": [19000, 15000], "elevation_a": 700, "elevation_b": 700, "site_modifier": True, "width": 1000})
    z = cad.result("site.elevation", {"id": site, "x": 10000, "y": 15000, "surface": "proposed"})["z"]
    assert z == approx(700, abs=1)
    pad = cad.result("pad.add", {"boundary": [[5000, 5000], [15000, 5000], [15000, 10000], [5000, 10000]], "elevation": 800, "retaining_edge": {"thickness": 250}})
    assert pad["wall"] and pad["quantities"]["length_m"] == approx(30.0)
    assert cad.result("site.elevation", {"id": site, "x": 10000, "y": 7500, "surface": "proposed"})["z"] == approx(800, abs=1)
