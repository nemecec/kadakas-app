"""Scenario: a landscape architect grades a site. Retaining walls follow the ground, grade objects
carry slopes, the site model shows a slope analysis with a legend, spot elevations read live
heights and a section line draws the profile."""

import math

from pytest import approx


def site_on_a_ramp(cad, n=11, step=1000, slope=0.1):
    """n by n loci, `step` mm apart. The ground rises toward +X by `slope`."""
    ids = []
    for i in range(n):
        for j in range(n):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * step, j * step, i * step * slope]}})
    return cad.result("site.create", {"source_ids": ids})["id"]


# --- Walls ------------------------------------------------------------------------------


def test_wall_on_flat_ground_has_exact_quantities(cad):
    w = cad.result("wall.add", {"path": [[0, 0], [4000, 0], [4000, 3000]], "thickness": 250, "top": {"mode": "fixed", "elevation": 1200}})
    q = w["quantities"]
    assert q["length_m"] == approx(7.0)
    assert q["face_area_m2"] == approx(7.0 * 1.2)
    assert q["volume_m3"] == approx(7.0 * 1.2 * 0.25)
    assert cad.result("objects.get", {"id": w["id"]})["shape"]["kind"] == "polyline"
    report = cad.result("wall.report")
    assert report["totals"]["walls"] == 1
    assert report["totals"]["volume_m3"] == approx(2.1)


def test_wall_follows_the_terrain_and_a_style(cad):
    site_on_a_ramp(cad)
    cad.result("wall_style.create", {"name": "Block wall", "components": [{"name": "Block", "material": "Concrete", "thickness": 200}, {"name": "Render", "material": "Lime", "thickness": 20}], "coping": {"height": 50, "overhang": 20}})
    w = cad.result("wall.add", {"path": [[1000, 2000], [6000, 2000]], "top": {"mode": "terrain", "offset": 600}, "embed": 100, "style": "Block wall"})
    q = w["quantities"]
    # The ground rises 100 mm per meter. The wall is 700 mm high at every place.
    assert q["min_height_mm"] == approx(700) and q["max_height_mm"] == approx(700)
    assert q["top_start_mm"] == approx(700) and q["top_end_mm"] == approx(1200)
    assert q["bottom_start_mm"] == approx(0) and q["bottom_end_mm"] == approx(500)
    assert q["thickness_mm"] == 220
    assert q["volume_m3"] == approx(5 * 0.7 * 0.22)
    assert q["coping_volume_m3"] == approx(5 * 0.26 * 0.05)
    # Change the style: walls follow.
    cad.result("wall_style.update", {"id": "Block wall", "components": [], "thickness": 300})
    assert cad.result("wall.report")["rows"][0]["thickness_mm"] == 300


def test_wall_in_the_3d_scene_stands_between_the_ground_and_the_top(cad):
    site_on_a_ramp(cad)
    w = cad.result("wall.add", {"path": [[1000, 2000], [6000, 2000]], "top": {"mode": "terrain", "offset": 600}, "thickness": 300})["id"]
    png = cad.result("render.png", {"width": 200, "height": 150})
    assert png["width"] == 200
    assert cad.result("objects.get", {"id": w})["data"]["landscape_wall"]["top"]["mode"] == "terrain"


def test_wall_as_site_modifier_is_a_retaining_edge(cad):
    site = site_on_a_ramp(cad)
    cad.result("wall.add", {"path": [[2000, 5000], [8000, 5000]], "thickness": 500, "top": {"mode": "fixed", "elevation": 1500}, "site_modifier": True, "backfill": 1000})
    z = lambda y: cad.result("site.elevation", {"id": site, "x": 5000, "y": y})["z"]
    # The ground behind the wall (left of the path) is at the top. Under the wall and on the low side it stays.
    assert z(5600) == approx(1500)
    assert z(5000) == approx(500)
    assert z(4600) == approx(500)
    # The wall is a structure: only the earth behind it counts as fill, and nothing is cut.
    cf = cad.result("site.cut_fill", {"id": site})
    assert cf["cut_m3"] == approx(0) and cf["fill_m3"] > 0
    # A higher pad of the user is not lowered by the wall.
    cad.execute({"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "pad", "elevation": 2500},
                                               "boundary": [[4000, 4000], [6000, 4000], [6000, 7000], [4000, 7000]]}})
    assert z(5600) == approx(2500)
    assert cad.result("site.cut_fill", {"id": site})["cut_m3"] == approx(0)
    # A terrain top cannot be a modifier.
    err = cad.error("wall.add", {"path": [[0, 0], [1000, 0]], "site_modifier": True})
    assert "fixed or sloped" in err["message"]


def test_wall_off_the_model_takes_the_edge_height(cad):
    site_on_a_ramp(cad)  # the ground rises 100 mm per meter over x 0 to 10 000
    w = cad.result("wall.add", {"path": [[-3000, 5000], [13000, 5000]], "top": {"mode": "terrain", "offset": 500}})["quantities"]
    assert w["partly_off_model"] is True
    assert w["bottom_start_mm"] == approx(0) and w["bottom_end_mm"] == approx(1000)
    assert "(off model)" in cad.result("render.svg")["svg"]


# --- Grade objects ----------------------------------------------------------------------


def test_grade_object_slope_math_and_moves(cad):
    g = cad.result("grade.add", {"a": [0, 0], "b": [10000, 0], "elevation_a": 1000, "slope": {"ratio": -50}})
    assert g["grade"]["percent"] == approx(-2.0)
    assert g["grade"]["elevation_b"] == approx(800)
    longer = cad.result("grade.set", {"id": g["id"], "b": [20000, 0]})
    assert longer["grade"]["elevation_b"] == approx(600)
    two = cad.result("grade.set", {"id": g["id"], "elevation_b": 1500, "slope": None})
    assert two["grade"]["percent"] == approx(2.5)
    assert two["grade"]["angle_deg"] == approx(math.degrees(math.atan(0.025)))
    svg = cad.result("render.svg")["svg"]
    assert "grade-arrow" in svg and "2.5% (1:40)" in svg


def test_grade_snaps_to_the_site_model(cad):
    site_on_a_ramp(cad)
    g = cad.result("grade.add", {"a": [1000, 1000], "b": [5000, 1000], "snap": "existing"})
    assert g["grade"]["elevation_a"] == approx(100) and g["grade"]["elevation_b"] == approx(500)
    assert g["grade"]["percent"] == approx(10.0)
    assert "elevation_b" in cad.error("grade.add", {"a": [0, 0], "b": [1, 0]})["message"]
    assert cad.error("grade.add", {"a": [-9000, 0], "b": [-8000, 0], "snap": "existing"})["code"] == 1


# --- Analysis ---------------------------------------------------------------------------


def test_slope_analysis_bands_cover_the_site(cad):
    site = site_on_a_ramp(cad)
    r = cad.result("site.analysis", {"id": site, "mode": "slope", "surface": "existing", "breaks": [2, 5, 10, 20],
                                     "legend": {"rect": [[12000, 0], [16000, 6000]], "show_areas": True}})
    bands = r["bands"]
    assert len(bands) == 5
    assert sum(b["area_m2"] for b in bands) == approx(100.0)
    assert sum(b["triangles"] for b in bands) > 0
    # A 10 percent ramp is in the band 10 to 20.
    assert bands[3]["percent"] == approx(100.0)
    svg = cad.result("render.svg")["svg"]
    assert 'data-kind="analysis"' in svg and "legend-swatch" in svg
    # Elevation bands.
    e = cad.result("site.analysis", {"id": site, "mode": "elevation", "breaks": []})
    assert len(e["bands"]) == 7
    assert sum(b["area_m2"] for b in e["bands"]) == approx(100.0)
    off = cad.result("site.analysis", {"id": site, "mode": "off"})
    assert "bands" not in off
    assert 'data-kind="analysis"' not in cad.result("render.svg")["svg"]
    assert cad.error("site.analysis", {"id": site, "breaks": [5, 2]})["code"] == -32602


# --- Spot elevations --------------------------------------------------------------------


def test_spot_elevation_reads_the_site_model_live(cad):
    site = site_on_a_ramp(cad)
    spot = cad.result("spot.add", {"at": [3000, 3000], "prefix": "TC"})
    assert spot["reading"]["existing"] == approx(300) and spot["reading"]["label"] == "TC 0.30"
    cad.execute({"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "pad", "elevation": 900},
                                               "boundary": [[2000, 2000], [4000, 2000], [4000, 4000], [2000, 4000]]}})
    got = cad.result("spot.get", {"id": spot["id"]})
    assert got["existing"] == approx(300) and got["proposed"] == approx(900)
    assert got["label"] == "TC EX 0.30 / PR 0.90"
    z = cad.result("site.elevation", {"id": site, "x": 3000, "y": 3000})["z"]
    assert got["proposed"] == approx(z)
    assert cad.result("spot.get", {"at": [-4000, 0]})["existing"] is None


def test_stake_shows_northing_and_easting(cad):
    site_on_a_ramp(cad)
    stake = cad.result("spot.add", {"at": [3000, 4000], "kind": "stake", "name": "S1"})
    assert stake["reading"]["georeferenced"] is False
    cad.execute({"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [500000.0, 6700000.0], "rotation": 0.0}})
    got = cad.result("spot.get", {"id": stake["id"]})
    assert got["georeferenced"] is True
    assert got["northing_m"] == approx(6700004.0) and got["easting_m"] == approx(500003.0)
    assert got["label"].startswith("S1 N 6700004.000 E 500003.000")


# --- Sections ---------------------------------------------------------------------------


def test_section_profile_equals_the_site_model_and_places_a_drawing(cad):
    site = site_on_a_ramp(cad)
    cad.result("wall.add", {"path": [[3000, 0], [3000, 10000]], "thickness": 300, "top": {"mode": "terrain", "offset": 800}})
    cad.execute({"op": "add_object", "shape": {"kind": "plant", "at": [7000, 5500], "species": "Oak", "spread": 3000, "height": 6000}})
    sec = cad.result("section.add", {"a": [0, 5000], "b": [10000, 5000], "name": "A", "exaggeration": 3})["id"]
    p = cad.result("section.profile", {"id": sec})
    assert p["length_mm"] == approx(10000)
    for s, z in p["existing"][0]:
        assert cad.result("site.elevation", {"id": site, "x": s, "y": 5000})["z"] == approx(z, abs=1e-6)
    assert p["off_model"] == []
    assert len(p["walls"]) == 1 and p["walls"][0]["top"] == approx(1100)
    assert len(p["plants"]) == 1
    sheet = cad.result("sheet.create", {"name": "S1", "paper": "a3"})["layer"]
    placed = cad.result("section.place", {"id": sec, "at": [20, 20], "layer": sheet})
    first = cad.result("objects.get", {"id": placed["ids"][0]})
    assert first["data"]["site_section_profile"]["section"] == sec
    assert first["shape"]["b"][0] == approx(120)  # 10 m at 1:100 is 100 mm on paper
    # Placing again replaces the first drawing in one step.
    n = len(cad.result("objects.list", {"layer": sheet}))
    again = cad.result("section.place", {"id": sec, "at": [20, 20], "layer": sheet})
    assert len(cad.result("objects.list", {"layer": sheet})) == n
    assert len(again["ids"]) == len(placed["ids"])
    cad.result("undo")
    assert len(cad.result("objects.list", {"layer": sheet})) == n


def test_section_beyond_the_model_keeps_the_exact_profile_on_it(cad):
    site = site_on_a_ramp(cad)
    sec = cad.result("section.add", {"a": [-2000, 5000], "b": [12000, 5000]})["id"]
    p = cad.result("section.profile", {"id": sec})
    (a0, a1), (b0, b1) = p["off_model"]
    assert a0 == approx(0) and a1 == approx(2000) and b0 == approx(12000) and b1 == approx(14000)
    run = p["existing"][0]
    assert run[0][1] == approx(0) and run[-1][1] == approx(1000)
    for s, z in run:
        if 2000 <= s <= 12000:
            assert cad.result("site.elevation", {"id": site, "x": s - 2000, "y": 5000})["z"] == approx(z, abs=1e-6)


def test_a_refused_grade_leaves_no_redo_step(cad):
    g = cad.result("grade.add", {"a": [0, 0], "b": [10000, 0], "elevation_a": 0, "elevation_b": 100})["id"]
    before = cad.result("document.get")
    assert cad.error("grade.set", {"id": g, "b": [0, 0]})["code"] == 1
    assert cad.error("grade.add", {"a": [0, 0], "b": [0, 0], "elevation_a": 1, "elevation_b": 2})["code"] == 1
    assert cad.result("document.get") == before
    assert cad.result("redo")["done"] is False


def test_describe_lists_the_grading_methods(cad):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    for m in ["wall.add", "wall.set", "wall.report", "wall_style.create", "grade.add", "grade.set", "grade.get", "site.analysis",
              "spot.add", "spot.set", "spot.get", "section.add", "section.set", "section.profile", "section.place"]:
        assert m in names
    assert len(d["grading"]["examples"]) >= 8
