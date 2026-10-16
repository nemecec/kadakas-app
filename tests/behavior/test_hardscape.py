"""Scenario: a landscape architect draws paved areas and paths with styles, plants beds with a
mix, drapes them on the site model, and reads the quantity reports."""

import math

from pytest import approx

from helpers import strip_next_id

SQUARE = [[0, 0], [4000, 0], [4000, 2500], [0, 2500]]


def setts(cad, **fields):
    params = {
        "name": "Granite setts", "material": "Granite", "thickness": 60,
        "components": [{"name": "Bedding", "material": "Sand", "thickness": 40},
                       {"name": "Base", "material": "Crushed stone", "thickness": 150}],
        "border": {"width": 200, "material": "Granite kerb"},
        "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5},
    }
    params.update(fields)
    return cad.result("hardscape_style.create", params)["id"]


def plants(cad):
    a = cad.result("plant_style.create", {"name": "Lavender", "id_code": "LA", "spread": 500, "price": 4})["id"]
    b = cad.result("plant_style.create", {"name": "Sedge", "id_code": "SE", "category": "grass", "spread": 400, "price": 3})["id"]
    return a, b


def layer_volume(row, kind):
    return sum(layer["volume_m3"] for layer in row["layers"] if layer["kind"] == kind)


# --- Hardscape styles and quantities ---------------------------------------------------


def test_styled_slab_reports_areas_and_the_volume_of_every_layer(cad):
    sid = setts(cad)
    got = cad.result("hardscape_style.get", {"id": sid})
    assert got["total_thickness"] == 250 and got["used_by"] == 0
    h = cad.result("hardscape.add", {"boundary": SQUARE, "style": "Granite setts"})["id"]
    row = cad.result("report.hardscape")["rows"][0]
    assert row["id"] == h and row["style_name"] == "Granite setts" and row["mode"] == "slab"
    assert row["plan_area_m2"] == approx(10.0) and row["surface_area_m2"] == approx(10.0)
    assert row["perimeter_m"] == approx(13.0)
    # The border is 0.2 m wide round the slab and replaces part of the surface. The corners count
    # once: 10 m2 less the 3.6 x 2.1 m of the paved part.
    assert layer_volume(row, "border") == approx(2.44 * 0.06)
    assert layer_volume(row, "surface") == approx(7.56 * 0.06)
    assert layer_volume(row, "component") == approx(10 * 0.04 + 10 * 0.15)
    assert row["thickness_mm"] == 250
    assert cad.result("objects.get", {"id": h})["shape"]["thickness"] == 250  # the shape follows the style
    report = cad.result("report.hardscape")
    stone = [m for m in report["materials"] if m["material"] == "Crushed stone"][0]
    assert stone["volume_m3"] == approx(1.5)
    assert report["totals"]["plan_area_m2"] == approx(10.0)


def test_editing_the_style_updates_hardscapes_and_undo_reverts_it(cad):
    sid = setts(cad)
    h = cad.result("hardscape.add", {"boundary": SQUARE, "style": sid})["id"]
    cad.result("hardscape_style.update", {"id": sid, "thickness": 100, "border": None})
    row = cad.result("report.hardscape")["rows"][0]
    assert row["thickness_mm"] == 290
    assert not [layer for layer in row["layers"] if layer["kind"] == "border"]
    assert cad.result("objects.get", {"id": h})["shape"]["thickness"] == 290
    before = strip_next_id(cad.doc())
    cad.result("hardscape.set", {"id": h, "style": None})
    assert cad.result("objects.get", {"id": h})["data"]["hardscape"]["style"] is None
    assert cad.result("undo")["done"]
    assert strip_next_id(cad.doc()) == before


def test_hardscape_without_a_style_keeps_its_fields_and_old_files_load(cad):
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": SQUARE, "material": "Gravel", "thickness": 100}})
    row = cad.result("report.hardscape")["rows"][0]
    assert row["material"] == "Gravel" and row["name"] == "Gravel"
    assert row["layers"][0]["kind"] == "slab" and row["volume_m3"] == approx(1.0)
    doc = cad.doc()
    cad.result("document.load", {"document": doc})
    assert cad.result("report.hardscape")["rows"][0]["volume_m3"] == approx(1.0)
    assert cad.result("report.areas")["rows"][0]["area_m2"] == approx(10.0)


# --- Paths -----------------------------------------------------------------------------


def test_a_straight_path_has_the_area_length_times_width(cad):
    h = cad.result("hardscape.add", {"center": [[0, 0], [10000, 0]], "width": 1500})["id"]
    row = cad.result("report.hardscape")["rows"][0]
    assert row["mode"] == "path" and row["plan_area_m2"] == approx(15.0)
    obj = cad.result("objects.get", {"id": h})
    assert len(obj["shape"]["boundary"]) == 4
    cad.result("hardscape.set", {"id": h, "width": 2000})
    assert cad.result("report.hardscape")["rows"][0]["plan_area_m2"] == approx(20.0)
    # Moving the path moves its center line, so the width can still change afterwards.
    cad.execute({"op": "move_objects", "ids": [h], "delta": [500, 500]})
    cad.result("hardscape.set", {"id": h, "width": 1000})
    xs = [p[0] if isinstance(p, list) else p["p"][0] for p in cad.result("objects.get", {"id": h})["shape"]["boundary"]]
    assert min(xs) == approx(500) and max(xs) == approx(10500)


def test_curved_path_with_joins_and_a_one_sided_border(cad):
    sid = setts(cad, border={"width": 150, "material": "Kerb", "sides": "left"})
    center = [[0, 0], {"p": [6000, 0], "kind": "arc", "radius": 1500}, [6000, 5000]]
    areas = {}
    for join in ("miter", "round", "bevel"):
        cad.result("hardscape.add", {"center": center, "width": 1500, "join": join, "style": sid})
    for row in cad.result("report.hardscape")["rows"]:
        areas[row["id"]] = row["plan_area_m2"]
        border = [layer for layer in row["layers"] if layer["kind"] == "border"][0]
        # Only one side has the border: its length is the length of the center line.
        assert 0 < border["area_m2"] < 0.15 * 14.0 + 0.5
    values = sorted(areas.values())
    assert values[0] > 0 and values[-1] < values[0] * 1.2  # the joins differ only a little
    # A path outline cannot be edited by vertex: the center line is the source.
    path_id = list(areas)[0]
    assert cad.error("execute", {"command": {"op": "set_vertex", "id": path_id, "index": 0, "p": [1, 1]}})["code"] == 3


def test_path_input_is_checked(cad):
    assert cad.error("hardscape.add", {"center": [[0, 0], [0, 0]]})["code"] == 3
    assert cad.error("hardscape.add", {"center": [[0, 0], [10, 0]], "width": -5})["code"] == 3
    assert cad.error("hardscape.add", {"center": [[0, 0], [10, 0]], "join": "square"})["code"] == -32602
    assert cad.error("hardscape.add", {"mode": "path", "boundary": SQUARE})["code"] == -32602
    assert cad.error("hardscape.add", {})["code"] == 3
    assert cad.error("hardscape.add", {"boundary": SQUARE, "style": "none"})["code"] == -32602
    assert cad.error("hardscape.add", {"boundary": SQUARE, "slope": {"type": "edges", "elevations": [0, 1]}})["code"] == 3
    assert cad.result("objects.list", {"kind": "hardscape"}) == []


# --- Slopes ----------------------------------------------------------------------------


def test_slopes_make_the_surface_larger_than_the_plan(cad):
    h = cad.result("hardscape.add", {"boundary": [[0, 0], [2000, 0], [2000, 2000], [0, 2000]],
                                     "slope": {"type": "cross", "percent": 100}})["id"]
    row = cad.result("report.hardscape")["rows"][0]
    assert row["surface_area_m2"] == approx(4 * math.sqrt(2))
    cad.result("hardscape.set", {"id": h, "slope": {"type": "edges", "elevations": [0, 0, 1000, 1000]}})
    row = cad.result("report.hardscape")["rows"][0]
    assert row["surface_area_m2"] == approx(4 * math.sqrt(1.25))
    cad.result("hardscape.set", {"id": h, "slope": None})
    assert cad.result("report.hardscape")["rows"][0]["surface_area_m2"] == approx(4.0)
    assert cad.error("hardscape.set", {"id": h, "slope": {"type": "cross", "percent": 9999}})["code"] == 3


# --- Site model: drape and pad ----------------------------------------------------------


def ramp(cad):
    ids = [cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}})[0]
           for i in range(7) for j in range(7)]
    return cad.result("site.create", {"source_ids": ids})["id"]


def test_draping_puts_the_hardscape_on_the_ground(cad):
    site = ramp(cad)
    h = cad.result("hardscape.add", {"boundary": [[1000, 1000], [3000, 1000], [3000, 3000], [1000, 3000]],
                                     "thickness": 100, "elevation": 10, "drape": True})["id"]
    row = cad.result("report.hardscape")["rows"][0]
    assert row["draped"] and row["surface_area_m2"] == approx(4 * math.sqrt(1.01), rel=1e-6)
    # In 3D the slab rises with the ramp (100 mm per metre over 2 m).
    png = cad.result("render.png", {"view": "front", "width": 200, "height": 100})
    assert png["width"] == 200
    cad.result("hardscape.set", {"id": h, "drape": False})
    assert not cad.result("report.hardscape")["rows"][0]["draped"]
    assert cad.result("site.elevation", {"id": site, "x": 2000, "y": 2000})["z"] == approx(200)


def test_a_hardscape_can_act_as_a_pad_on_the_site_model(cad):
    site = ramp(cad)
    cad.result("hardscape.add", {"boundary": [[2000, 2000], [4000, 2000], [4000, 4000], [2000, 4000]],
                                 "thickness": 200, "elevation": 100, "site_modifier": True})
    # The formation level is the top (100) minus the build-up (200).
    assert cad.result("site.elevation", {"id": site, "x": 3000, "y": 3000})["z"] == approx(-100)
    assert cad.result("site.elevation", {"id": site, "x": 5500, "y": 3000})["z"] == approx(550)


# --- Landscape areas -------------------------------------------------------------------


def meadow(cad):
    plants(cad)
    return cad.result("landscape_area_style.create", {
        "name": "Meadow", "fields": {"hydrozone": "low"},
        "mix": [{"plant": "Lavender", "percent": 60, "density": 4}, {"plant": "Sedge", "percent": 40, "spacing": 500}],
    })["id"]


def bed(cad, **extra):
    params = {"boundary": [[0, 0], [10000, 0], [10000, 10000], [0, 10000]], "name": "Bed A", "style": "Meadow"}
    params.update(extra)
    return cad.result("landscape_area.add", params)["id"]


def test_quantities_per_plant_come_from_the_mix_minus_holes(cad):
    meadow(cad)
    area = bed(cad)
    row = cad.result("report.landscape_areas")["rows"][0]
    assert row["id"] == area and row["fields"] == {"hydrozone": "low"}
    assert row["net_area_m2"] == approx(100.0)
    counts = {p["name"]: p["count"] for p in row["rows"]}
    assert counts == {"Lavender": 240, "Sedge": 160}  # 60 m2 at 4 per m2, 40 m2 at 0.5 m spacing
    # A pond of 4 m2 stays empty.
    cad.result("landscape_area.set", {"id": area, "holes": [[[1000, 1000], [3000, 1000], [3000, 3000], [1000, 3000]]]})
    row = cad.result("report.landscape_areas")["rows"][0]
    assert row["net_area_m2"] == approx(96.0) and row["holes_area_m2"] == approx(4.0)
    counts = {p["name"]: p["count"] for p in row["rows"]}
    assert counts == {"Lavender": 230, "Sedge": 154}
    # Holes from other objects, and a minimum count.
    pond = cad.execute({"op": "add_object", "shape": {"kind": "rect", "min": [5000, 5000], "max": [6000, 6000]}})[0]
    cad.result("landscape_area.set", {"id": area, "exclude": [pond]})
    assert cad.result("report.landscape_areas")["rows"][0]["net_area_m2"] == approx(99.0)
    assert cad.error("landscape_area.set", {"id": area, "holes": [[[-5, 0], [10, 0], [10, 10]]]})["code"] == 3


def test_the_plant_list_adds_the_plants_of_areas_to_single_plants(cad):
    meadow(cad)
    bed(cad)
    cad.result("plants.place", {"mode": "single", "style": "Lavender", "at": [100, 100]})
    cad.result("plants.place", {"mode": "single", "style": "Lavender", "at": [300, 100], "overrides": {"price": 9}})
    plain = cad.result("report.plants")["rows"]
    assert sum(r["count"] for r in plain) == 2  # report.plants lists single plants only
    pl = cad.result("report.plant_list", {"format": "csv"})
    rows = {r["name"]: r for r in pl["rows"]}
    assert (rows["Lavender"]["plants"], rows["Lavender"]["in_areas"], rows["Lavender"]["count"]) == (2, 240, 242)
    assert rows["Sedge"]["count"] == 160 and rows["Sedge"]["areas"][0]["name"] == "Bed A"
    assert pl["total_count"] == 402 and pl["total_price"] == approx(4 * 240 + 3 * 160 + 4 + 9)
    assert pl["csv"].splitlines()[0].startswith("Name,ID,Botanical name")
    # Plant styles count the mixes that use them, and cannot be deleted while a mix needs them.
    assert cad.result("plant_style.get", {"id": "Sedge"})["used_by"] == 1
    assert cad.error("plant_style.delete", {"id": "Sedge"})["code"] == 5
    cad.result("plant_style.delete", {"id": "Sedge", "reassign": "Lavender"})
    style = cad.result("landscape_area_style.get", {"id": "Meadow"})
    assert [e["percent"] for e in style["mix"]] == [100.0]


def test_plant_parity_features_work_with_area_mixes(cad):
    # A tree style with a protection zone, a groundcover counted by area, and locked fields.
    cad.result("plant_style.create", {"name": "Oak", "id_code": "QU", "category": "tree", "spread": 6000, "height": 14000,
                                      "trunk_diameter": 400, "protection_zone": True, "locked": ["spread"], "price": 200})
    cad.result("plant_style.create", {"name": "Thyme", "id_code": "TH", "category": "groundcover", "spread": 300,
                                      "quantity": "area", "price": 12})
    cad.result("landscape_area_style.create", {"name": "Grove", "show_plants": True, "mix": [
        {"plant": "Oak", "percent": 10, "density": 0.05, "min_count": 2}, {"plant": "Thyme", "percent": 90}]})
    cad.result("landscape_area.add", {"boundary": [[0, 0], [20000, 0], [20000, 10000], [0, 10000]], "name": "Grove", "style": "Grove"})
    # Single oaks keep the locked spread when sizes vary, and the list counts both kinds of oak.
    cad.result("plants.place", {"mode": "grid", "style": "Oak", "min": [0, 0], "max": [3000, 3000], "spacing": 3000,
                                "variation": {"seed": 3, "size": 0.4, "rotation": True}})
    assert {o["shape"]["spread"] for o in cad.result("objects.list", {"kind": "plant"})} == {6000}
    rows = {r["name"]: r for r in cad.result("report.plant_list")["rows"]}
    assert rows["Oak"]["in_areas"] == 2  # 20 m2 at 0.05 per m2 is one tree: the minimum count of 2 applies
    assert rows["Oak"]["plants"] == 4
    # The groundcover counts by area: 90 percent of 200 m2.
    assert rows["Thyme"]["quantity"] == "area" and rows["Thyme"]["area_m2"] == approx(180.0)
    assert rows["Thyme"]["total_price"] == approx(12 * 180.0)
    # The 2D drawing shows the scattered plants and the protection zones of the trees.
    svg = cad.result("render.svg", {"width": 800})["svg"]
    assert 'data-part="plants"' in svg and 'data-part="rpz"' in svg


# --- 2D drawing -------------------------------------------------------------------------


def test_the_plan_shows_joint_patterns_borders_holes_and_plants(cad):
    setts(cad)
    cad.result("hardscape.add", {"boundary": SQUARE, "style": "Granite setts"})
    meadow(cad)
    bed(cad, show_plants=True, holes=[[[1000, 1000], [2000, 1000], [2000, 2000]]])
    svg = cad.result("render.svg", {"width": 1000})["svg"]
    assert 'data-part="joints"' in svg and 'data-pattern="running_bond"' in svg
    assert 'data-part="border"' in svg and 'data-part="hole"' in svg and 'data-part="plants"' in svg
    again = cad.result("render.svg", {"width": 1000})["svg"]
    assert again == svg  # the scatter is deterministic
    cad.result("landscape_area.set", {"id": cad.result("objects.list", {"kind": "landscape_area"})[0]["id"], "show_plants": False})
    assert 'data-part="plants"' not in cad.result("render.svg", {"width": 1000})["svg"]


def test_joint_pattern_kinds(cad):
    for kind in ("running_bond", "stack", "herringbone", "basket_weave"):
        sid = cad.result("hardscape_style.create", {"name": kind, "joints": {"kind": kind, "unit_length": 200, "unit_width": 100, "angle": 30}})["id"]
        cad.result("hardscape.add", {"boundary": SQUARE, "style": sid})
    svg = cad.result("render.svg", {"width": 600})["svg"]
    for kind in ("running_bond", "stack", "herringbone", "basket_weave"):
        assert f'data-pattern="{kind}"' in svg
    assert cad.error("hardscape_style.create", {"name": "bad", "joints": {"kind": "zigzag"}})["code"] == -32602
    assert cad.error("hardscape_style.create", {"name": "bad", "joints": {"unit_length": 1}})["code"] == -32602


def test_describe_lists_the_methods_and_examples(cad):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    for name in ["hardscape_style.create", "landscape_area_style.create", "hardscape.add", "hardscape.set", "landscape_area.add",
                 "landscape_area.set", "report.hardscape", "report.landscape_areas", "report.plant_list"]:
        assert name in names
    assert len(d["hardscapes"]["examples"]) >= 6


# --- Review fixes: path geometry, alignment, partial drape, limits -------------------------


def test_tight_curves_give_a_clean_outline_and_closed_paths_are_rings(cad):
    # A U-turn with a radius of 300 mm and a width of 1000 mm: the old outline crossed itself.
    pts = [[-3000, 300], [0, 300]]
    n = 48
    pts += [[300 * math.cos(math.pi / 2 - math.pi * k / n), 300 * math.sin(math.pi / 2 - math.pi * k / n)] for k in range(1, n)]
    pts += [[0, -300], [-3000, -300]]
    cad.result("hardscape.add", {"center": pts, "width": 1000, "join": "round"})
    area = cad.result("report.hardscape")["rows"][0]["plan_area_m2"]
    # Two strips of 3 x 1 m fold over each other on 0.6 m: 4.8 m2 plus the half disc of the turn.
    assert 5.5 < area < 6.5, area
    ring = cad.result("hardscape.add", {"center": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "width": 1000, "closed": True})["id"]
    row = [r for r in cad.result("report.hardscape")["rows"] if r["id"] == ring][0]
    assert row["plan_area_m2"] == approx(16.0, rel=1e-4)  # 5 x 5 m minus 3 x 3 m


def test_path_alignment_and_offset(cad):
    h = cad.result("hardscape.add", {"center": [[0, 0], [10000, 0]], "width": 1000})["id"]

    def ys():
        pts = cad.result("objects.get", {"id": h})["shape"]["boundary"]
        values = [p[1] if isinstance(p, list) else p["p"][1] for p in pts]
        return round(min(values)), round(max(values))

    assert ys() == (-500, 500)
    cad.result("hardscape.set", {"id": h, "align": "left"})
    assert ys() == (-1000, 0)
    cad.result("hardscape.set", {"id": h, "align": "right"})
    assert ys() == (0, 1000)
    cad.result("hardscape.set", {"id": h, "align": "center", "offset": 300})
    assert ys() == (-200, 800)
    assert cad.error("hardscape.set", {"id": h, "align": "top"})["code"] == -32602


def test_slope_by_angle_and_by_rise_and_run(cad):
    h = cad.result("hardscape.add", {"boundary": [[0, 0], [2000, 0], [2000, 2000], [0, 2000]],
                                     "slope": {"type": "rise_run", "rise": 1, "run": 1}})["id"]
    assert cad.result("report.hardscape")["rows"][0]["surface_area_m2"] == approx(4 * math.sqrt(2))
    cad.result("hardscape.set", {"id": h, "slope": {"type": "angle", "degrees": 30, "angle_deg": 90}})
    assert cad.result("report.hardscape")["rows"][0]["surface_area_m2"] == approx(4 * math.sqrt(1 + math.tan(math.radians(30)) ** 2))
    assert cad.error("hardscape.set", {"id": h, "slope": {"type": "rise_run", "rise": 1, "run": 0}})["code"] == 3
    assert cad.error("hardscape.set", {"id": h, "slope": {"type": "angle", "degrees": 80}})["code"] == 3


def test_edge_elevations_survive_an_outline_edit(cad):
    h = cad.result("hardscape.add", {"boundary": [[0, 0], [2000, 0], [2000, 2000], [0, 2000]],
                                     "slope": {"type": "edges", "elevations": [0, 0, 200, 200]}})["id"]
    before = cad.result("report.hardscape")["rows"][0]["surface_area_m2"]
    shape = cad.result("objects.get", {"id": h})["shape"]
    shape["boundary"].insert(1, [1000, 0])
    cad.execute({"op": "modify_object", "id": h, "shape": shape})
    obj = cad.result("objects.get", {"id": h})
    assert len(obj["data"]["hardscape"]["slope"]["elevations"]) == 5
    assert cad.result("report.hardscape")["rows"][0]["surface_area_m2"] == approx(before)
    cad.result("hardscape.set", {"id": h, "elevation": 5})  # later changes still work


def test_a_hardscape_half_on_the_site_model_is_partially_draped(cad):
    ramp(cad)  # z = x / 10 over x = 0 to 6000
    cad.result("hardscape.add", {"boundary": [[5000, 1000], [7000, 1000], [7000, 3000], [5000, 3000]], "thickness": 100, "drape": True})
    row = cad.result("report.hardscape")["rows"][0]
    assert row["draped"] and row["partially_draped"]
    assert row["surface_area_m2"] == approx(2 * math.sqrt(1.01) + 2, rel=1e-6)
    assert row["volume_m3"] == approx(row["surface_area_m2"] * 0.1, rel=1e-6)


def test_overlapping_holes_count_once_and_holes_stay_inside(cad):
    meadow(cad)
    area = bed(cad)
    cad.result("landscape_area.set", {"id": area, "holes": [
        [[1000, 1000], [5000, 1000], [5000, 5000], [1000, 5000]],
        [[3000, 3000], [7000, 3000], [7000, 7000], [3000, 7000]]]})
    row = cad.result("report.landscape_areas")["rows"][0]
    assert row["holes_area_m2"] == approx(16 + 16 - 4)
    assert row["net_area_m2"] == approx(100 - 28)
    # The area cannot shrink so that a hole falls outside.
    shape = cad.result("objects.get", {"id": area})["shape"]
    shape["boundary"] = [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]
    err = cad.error("execute", {"command": {"op": "modify_object", "id": area, "shape": shape}})
    assert "holes" in err["message"]


def test_a_huge_boundary_is_refused_and_fine_joints_stay_fast(cad):
    import time
    n = 60000
    boundary = [[1000 * math.cos(2 * math.pi * k / n), 1000 * math.sin(2 * math.pi * k / n)] for k in range(n)]
    assert "at most" in cad.error("hardscape.add", {"boundary": boundary})["message"]
    sid = cad.result("hardscape_style.create", {"name": "Fine", "joints": {"kind": "running_bond", "unit_length": 12, "unit_width": 10, "spacing": 1}})["id"]
    n = 20000
    ring = [[50000 * math.cos(2 * math.pi * k / n), 50000 * math.sin(2 * math.pi * k / n)] for k in range(n)]
    cad.result("hardscape.add", {"boundary": ring, "style": sid})
    start = time.monotonic()
    cad.result("render.svg", {"width": 400})
    assert time.monotonic() - start < 5


def test_relinking_a_plant_to_a_style_that_now_locks_a_field(cad):
    plants(cad)
    cad.result("plants.place", {"mode": "single", "style": "Lavender", "at": [0, 0], "overrides": {"spread": 777, "price": 9}})
    plant_id = cad.result("objects.list", {"kind": "plant"})[0]["id"]
    cad.result("plant_style.update", {"id": "Lavender", "locked": ["spread"]})
    cad.result("plants.set_style", {"ids": [plant_id], "style": "Lavender"})
    overrides = cad.result("objects.get", {"id": plant_id})["data"]["plant"]["overrides"]
    assert "spread" not in overrides and overrides["price"] == 9


def test_hardscape_style_name_material_and_criteria(cad):
    sid = setts(cad, name="Plaza granite")
    cad.result("hardscape.add", {"boundary": SQUARE, "style": sid})
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": SQUARE, "material": "Gravel"}})
    rows = cad.result("objects.query", {"criteria": 'Kind = "hardscape"', "columns": ["Style", "Name", "Material"]})
    assert sorted(r["values"] for r in rows["rows"]) == [["Gravel", "Gravel", "Gravel"], ["Plaza granite", "Plaza granite", "Granite"]]
    assert cad.result("objects.query", {"criteria": 'Style = "Plaza granite"'})["count"] == 1
    assert cad.result("objects.query", {"criteria": 'Material = "Granite"'})["count"] == 1
    assert sorted(r["name"] for r in cad.result("report.areas")["rows"]) == ["Gravel", "Plaza granite"]
