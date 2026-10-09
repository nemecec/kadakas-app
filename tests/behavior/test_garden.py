"""Scenario: a landscape designer draws a small garden on two layers and reads the reports."""

import pytest
from pytest import approx

from helpers import plant_cmd


def build_garden(cad):
    site = cad.execute({"op": "add_layer", "name": "Site"})[0]
    planting = cad.execute({"op": "add_layer", "name": "Planting"})[0]

    cad.execute({"op": "set_active", "layer": site})
    patio = cad.execute({
        "op": "add_object",
        "shape": {"kind": "hardscape", "boundary": [[0, 0], [3000, 0], [3000, 1200], [0, 1200]],
                  "material": "Granite setts"},
    })[0]
    meadow = cad.execute({
        "op": "add_object",
        "shape": {"kind": "landscape_area", "boundary": [[0, 2000], [5000, 2000], [5000, 4000], [0, 4000]],
                  "name": "Wild meadow"},
    })[0]

    cad.execute({"op": "set_active", "layer": planting})
    plants = []
    for i in range(3):
        plants += cad.execute(plant_cmd(500 + i * 1000, 3000, species="Acer campestre", spread=1500, height=4000))
    for i in range(2):
        plants += cad.execute(plant_cmd(500 + i * 1000, 3500, species="Lavandula angustifolia", spread=400, height=600))
    return {"site": site, "planting": planting, "patio": patio, "meadow": meadow, "plants": plants}


def test_plant_schedule_counts_each_species(cad):
    build_garden(cad)
    rows = cad.result("report.plants")["rows"]
    assert rows == [
        {"species": "Acer campestre", "count": 3},
        {"species": "Lavandula angustifolia", "count": 2},
    ]


def test_area_report_gives_square_metres_for_hardscape_and_landscape(cad):
    ids = build_garden(cad)
    rows = {row["id"]: row for row in cad.result("report.areas")["rows"]}
    assert set(rows) == {ids["patio"], ids["meadow"]}, "only paved and planting areas are reported"

    assert rows[ids["patio"]]["kind"] == "hardscape"
    assert rows[ids["patio"]]["name"] == "Granite setts"
    assert rows[ids["patio"]]["area_m2"] == approx(3.6)  # 3000 x 1200 mm

    assert rows[ids["meadow"]]["kind"] == "landscape_area"
    assert rows[ids["meadow"]]["name"] == "Wild meadow"
    assert rows[ids["meadow"]]["area_m2"] == approx(10.0)  # 5000 x 2000 mm


def test_objects_stay_on_the_layer_that_was_active_when_drawn(cad):
    ids = build_garden(cad)
    on_site = {o["id"] for o in cad.result("objects.list", {"layer": ids["site"]})}
    on_planting = {o["id"] for o in cad.result("objects.list", {"layer": ids["planting"]})}
    assert on_site == {ids["patio"], ids["meadow"]}
    assert on_planting == set(ids["plants"])


def test_summary_counts_objects_by_kind(cad):
    build_garden(cad)
    summary = cad.result("document.summary")
    assert summary["object_count"] == 7
    assert summary["objects_by_kind"] == {"hardscape": 1, "landscape_area": 1, "plant": 5}
    assert summary["can_undo"] is True
    assert summary["can_redo"] is False


@pytest.mark.parametrize(
    ("width_mm", "depth_mm", "expected_m2"),
    [(3000, 1200, 3.6), (1000, 1000, 1.0), (500, 200, 0.1), (12000, 8000, 96.0)],
)
def test_rectangle_area_converts_mm_squared_to_m2(cad, width_mm, depth_mm, expected_m2):
    cad.execute({
        "op": "add_object",
        "shape": {"kind": "hardscape", "boundary": [[0, 0], [width_mm, 0], [width_mm, depth_mm], [0, depth_mm]],
                  "material": "Gravel"},
    })
    (row,) = cad.result("report.areas")["rows"]
    assert row["area_m2"] == approx(expected_m2)


@pytest.mark.parametrize("points", [
    [[0, 0], [3000, 0], [3000, 1200], [0, 1200]],  # counterclockwise
    [[0, 0], [0, 1200], [3000, 1200], [3000, 0]],  # clockwise
])
def test_area_does_not_depend_on_outline_winding(cad, points):
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": points, "material": "Setts"}})
    (row,) = cad.result("report.areas")["rows"]
    assert row["area_m2"] == approx(3.6)


def test_circles_and_plants_are_not_in_the_area_report(cad):
    cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 1000}})
    cad.execute(plant_cmd(0, 0))
    assert cad.result("report.areas") == {"rows": []}


def test_empty_document_reports_are_empty(cad):
    assert cad.result("report.plants") == {"rows": []}
    assert cad.result("report.areas") == {"rows": []}
    assert cad.result("document.summary")["bounds"] is None
