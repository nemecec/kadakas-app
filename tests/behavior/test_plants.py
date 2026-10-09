"""Scenario: a landscape architect builds a plant catalog, places plants in every mode,
reads the schedule, and checks existing trees."""

import csv
import io

import pytest
from pytest import approx

SQUARE = [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]


def make_style(cad, name="Field maple", **fields):
    params = {"name": name, "botanical_name": "Acer campestre", "id_code": "AC", "category": "tree",
              "spread": 7000, "height": 12000, "graphic": "cloud", "form": "spreading", "price": 100}
    params.update(fields)
    return cad.result("plant_style.create", params)["id"]


def plant_objects(cad):
    return cad.result("objects.list", {"kind": "plant"})


def place(cad, **params):
    return cad.result("plants.place", params)


# --- Plant styles ---------------------------------------------------------------------


def test_create_list_get_update_duplicate_and_delete_styles(cad):
    sid = make_style(cad)
    listing = cad.result("plant_style.list")["styles"]
    assert [(s["id"], s["name"], s["used_by"]) for s in listing] == [(sid, "Field maple", 0)]
    got = cad.result("plant_style.get", {"id": "field maple"})
    assert got["botanical_name"] == "Acer campestre"
    assert got["category"] == "tree" and got["graphic"] == "cloud" and got["form"] == "spreading"

    updated = cad.result("plant_style.update", {"id": sid, "name": "Maple", "spread": 5000, "custom": {"water": "low"}})
    assert updated["name"] == "Maple" and updated["spread"] == 5000 and updated["custom"] == {"water": "low"}

    copy_id = cad.result("plant_style.duplicate", {"id": "Maple"})["id"]
    assert cad.result("plant_style.get", {"id": copy_id})["name"] == "Maple copy"
    cad.result("plant_style.delete", {"id": copy_id})
    assert len(cad.result("plant_style.list")["styles"]) == 1


def test_bad_style_input_is_refused(cad):
    make_style(cad)
    assert cad.error("plant_style.create", {"name": "Field maple"})["code"] == 1  # duplicate name
    assert cad.error("plant_style.create", {"name": "X", "spread": -5})["code"] == -32602
    assert cad.error("plant_style.create", {"name": "X", "category": "cactus"})["code"] == -32602
    assert cad.error("plant_style.create", {"name": "X", "colour": "red"})["code"] == -32602
    assert cad.error("plant_style.get", {"id": "nope"})["code"] == -32602


def test_undo_reverts_style_changes_one_step_at_a_time(cad):
    sid = make_style(cad)
    cad.result("plant_style.update", {"id": sid, "spread": 3000})
    assert cad.result("undo")["done"]
    assert cad.result("plant_style.get", {"id": sid})["spread"] == 7000
    assert cad.result("undo")["done"]
    assert cad.result("plant_style.list")["styles"] == []


# --- Styles and plants ----------------------------------------------------------------


def test_plants_follow_their_style_but_keep_their_overrides(cad):
    sid = make_style(cad)
    plain = place(cad, mode="single", style=sid, at=[0, 0])["created"][0]
    own = place(cad, mode="single", style=sid, at=[9000, 0], overrides={"spread": 2000})["created"][0]
    assert cad.result("objects.get", {"id": plain})["shape"]["spread"] == 7000
    cad.result("plant_style.update", {"id": sid, "spread": 6000, "height": 9000})
    a, b = cad.result("objects.get", {"id": plain}), cad.result("objects.get", {"id": own})
    assert (a["shape"]["spread"], a["shape"]["height"]) == (6000, 9000)
    assert (b["shape"]["spread"], b["shape"]["height"]) == (2000, 9000)
    assert a["shape"]["species"] == "Acer campestre"
    assert a["data"]["plant"]["style"] == sid


def test_editing_a_styled_plant_directly_makes_an_override(cad):
    sid = make_style(cad)
    pid = place(cad, mode="single", style=sid, at=[0, 0])["created"][0]
    shape = cad.result("objects.get", {"id": pid})["shape"]
    shape["spread"] = 1234
    cad.execute({"op": "modify_object", "id": pid, "shape": shape})
    cad.result("plant_style.update", {"id": sid, "spread": 9000})
    obj = cad.result("objects.get", {"id": pid})
    assert obj["shape"]["spread"] == 1234
    assert obj["data"]["plant"]["overrides"] == {"spread": 1234}


def test_set_style_links_and_unlinks_existing_plants(cad):
    sid = make_style(cad)
    pid = cad.execute({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 800, "height": 900}})[0]
    cad.result("plants.set_style", {"ids": [pid], "style": "Field maple"})
    assert cad.result("objects.get", {"id": pid})["shape"]["spread"] == 7000
    cad.result("plants.set_style", {"ids": [pid], "unlink": True})
    obj = cad.result("objects.get", {"id": pid})
    assert "plant" not in obj.get("data", {}) and obj["shape"]["spread"] == 7000
    assert sid


def test_deleting_a_style_in_use_is_refused_and_reassign_moves_the_plants(cad):
    maple = make_style(cad)
    birch = make_style(cad, "Birch", botanical_name="Betula", id_code="BP", spread=3000)
    place(cad, mode="poly", style=maple, points=[[0, 0], [1000, 0]])
    err = cad.error("plant_style.delete", {"id": maple})
    assert "used by 2" in err["message"]
    assert len(cad.result("plant_style.list")["styles"]) == 2
    cad.result("plant_style.delete", {"id": maple, "reassign": birch})
    rows = cad.result("report.plant_schedule")["rows"]
    assert [(r["name"], r["count"]) for r in rows] == [("Birch", 2)]
    assert all(o["shape"]["spread"] == 3000 for o in plant_objects(cad))
    # One undo brings the deleted style and the old links back.
    assert cad.result("undo")["done"]
    assert [(r["name"], r["count"]) for r in cad.result("report.plant_schedule")["rows"]] == [("Field maple", 2)]


# --- Placement modes ------------------------------------------------------------------


def test_single_mode(cad):
    sid = make_style(cad)
    r = place(cad, mode="single", style=sid, at=[100, 200])
    assert r["count"] == 1
    assert plant_objects(cad)[0]["shape"]["at"] == [100, 200]


def test_path_mode_by_spacing_count_and_both(cad):
    sid = make_style(cad)
    path = [[0, 0], [1000, 0], [1000, 1000]]
    assert place(cad, mode="path", style=sid, path=path, spacing=500)["count"] == 5
    assert place(cad, mode="path", style=sid, path=path, count=4)["count"] == 4
    assert place(cad, mode="path", style=sid, path=path, count=3, spacing=300)["count"] == 3
    xs = [o["shape"]["at"] for o in plant_objects(cad)[:5]]
    assert xs[:3] == [[0, 0], [500, 0], [1000, 0]]


def test_path_mode_refuses_what_does_not_fit(cad):
    sid = make_style(cad)
    err = cad.error("plants.place", {"mode": "path", "style": sid, "path": [[0, 0], [100, 0]], "count": 5, "spacing": 100})
    assert err["code"] == -32602 and "path" in err["message"]
    assert plant_objects(cad) == []


def test_grid_and_triangular_modes(cad):
    sid = make_style(cad)
    assert place(cad, mode="grid", style=sid, min=[0, 0], max=[2000, 1000], spacing_x=1000, spacing_y=500)["count"] == 9
    assert place(cad, mode="triangular", style=sid, min=[0, 0], max=[2000, 2000], spacing=1000)["count"] == 3 + 2 + 3
    ats = [o["shape"]["at"] for o in plant_objects(cad)[9:]]
    odd_row = [a for a in ats if abs(a[1] - 866.0254) < 1]
    assert sorted(a[0] for a in odd_row) == [500, 1500]


def test_fill_mode_by_spacing_count_and_density_keeps_plants_inside(cad):
    sid = make_style(cad)
    by_spacing = place(cad, mode="fill", style=sid, boundary=SQUARE, spacing=1000, pattern="rectangular")
    assert by_spacing["count"] == 25
    by_count = place(cad, mode="fill", style=sid, boundary=SQUARE, count=37)
    assert by_count["count"] == 37
    by_density = place(cad, mode="fill", style=sid, boundary=SQUARE, density=1, pattern="rectangular")
    assert by_density["count"] == 25  # 1 per m2 is a 1000 mm grid
    for o in plant_objects(cad):
        x, y = o["shape"]["at"]
        assert -1e-6 <= x <= 4000 + 1e-6 and -1e-6 <= y <= 4000 + 1e-6


def test_fill_with_margin_keeps_plants_away_from_the_edge(cad):
    sid = make_style(cad)
    place(cad, mode="fill", style=sid, boundary=SQUARE, spacing=500, margin=300)
    for o in plant_objects(cad):
        x, y = o["shape"]["at"]
        assert 300 - 1e-6 <= x <= 3700 + 1e-6 and 300 - 1e-6 <= y <= 3700 + 1e-6


def test_fill_keeps_the_spacing_between_plants(cad):
    sid = make_style(cad)
    place(cad, mode="fill", style=sid, boundary=SQUARE, spacing=700)
    pts = [o["shape"]["at"] for o in plant_objects(cad)]
    assert len(pts) > 20
    smallest = min(((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5 for i, a in enumerate(pts) for b in pts[i + 1:])
    assert smallest >= 700 - 1e-6


def test_poly_mode_and_source_objects(cad):
    sid = make_style(cad)
    assert place(cad, mode="poly", style=sid, points=[[0, 0], [10, 10], [20, 0]])["count"] == 3
    bed = cad.execute({"op": "add_object", "shape": {"kind": "landscape_area", "name": "Bed", "boundary": SQUARE}})[0]
    assert place(cad, mode="fill", style=sid, source=bed, spacing=2000, pattern="rectangular")["count"] == 9
    edge = cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": False, "points": [[0, 0], [3000, 0]]}})[0]
    assert place(cad, mode="path", style=sid, source=edge, spacing=1000)["count"] == 4


def test_placement_errors_leave_the_document_unchanged(cad):
    sid = make_style(cad)
    for params in [
        {"mode": "spiral"},
        {"mode": "grid", "min": [0, 0], "max": [100, 100]},
        {"mode": "fill", "boundary": SQUARE, "spacing": 1},
        {"mode": "fill", "boundary": SQUARE, "count": 5, "spacing": 500},
        {"mode": "grid", "min": [0, 0], "max": [1e6, 1e6], "spacing": 100},
    ]:
        assert cad.error("plants.place", {"style": sid, **params})["code"] == -32602
    assert plant_objects(cad) == []


def test_one_undo_step_removes_a_whole_placement(cad):
    sid = make_style(cad)
    place(cad, mode="fill", style=sid, boundary=SQUARE, spacing=1000, tag=True)
    assert len(plant_objects(cad)) > 10
    assert cad.result("undo")["done"]
    assert plant_objects(cad) == []
    assert cad.result("objects.list", {"kind": "plant_tag"}) == []
    assert cad.result("redo")["done"]
    assert len(cad.result("objects.list", {"kind": "plant_tag"})) == 1


# --- Tags -----------------------------------------------------------------------------


def test_plant_tag_shows_code_and_count_in_the_svg(cad):
    sid = make_style(cad)
    r = place(cad, mode="poly", style=sid, points=[[0, 0], [2000, 0], [4000, 0]], tag=True)
    assert r["tag"] is not None
    assert ">AC (3)</text>" in cad.result("render.svg")["svg"]
    place(cad, mode="single", style=sid, at=[0, 3000])
    assert ">AC (4)</text>" in cad.result("render.svg")["svg"]


def test_tag_is_a_shape_with_an_editable_position(cad):
    sid = make_style(cad)
    tag = cad.execute({"op": "add_object", "shape": {"kind": "plant_tag", "at": [0, 1000], "anchor": [0, 0], "style": sid}})[0]
    cad.execute({"op": "move_objects", "ids": [tag], "delta": [50, 50]})
    shape = cad.result("objects.get", {"id": tag})["shape"]
    assert shape["at"] == [50, 1050] and shape["anchor"] == [50, 50]
    cad.execute({"op": "modify_object", "id": tag, "shape": {**shape, "at": [300, 2000]}})
    assert cad.result("objects.get", {"id": tag})["shape"]["at"] == [300, 2000]
    err = cad.error("execute", {"command": {"op": "add_object", "shape": {"kind": "plant_tag", "at": [0, 0], "anchor": [0, 0], "style": 9999}}})
    assert err["code"] == 1


def test_a_style_with_tags_cannot_be_deleted(cad):
    sid = make_style(cad)
    cad.execute({"op": "add_object", "shape": {"kind": "plant_tag", "at": [0, 1000], "anchor": [0, 0], "style": sid}})
    assert "used by 1" in cad.error("plant_style.delete", {"id": sid})["message"]


# --- Schedule -------------------------------------------------------------------------


def test_schedule_counts_and_prices_including_overrides(cad):
    maple = make_style(cad)
    lav = make_style(cad, "Lavender", botanical_name="Lavandula", id_code="LA", category="perennial", spread=600, height=500, price=6.4, size="P9")
    place(cad, mode="poly", style=maple, points=[[0, 0], [1, 1], [2, 2]])
    place(cad, mode="poly", style=maple, points=[[10, 10]], overrides={"price": 40})
    place(cad, mode="grid", style=lav, min=[0, 0], max=[1000, 1000], spacing=500)
    cad.execute({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": "Quercus robur", "spread": 8000, "height": 15000}})
    sched = cad.result("report.plant_schedule")
    maples = [r for r in sched["rows"] if r["name"] == "Field maple"]
    assert sorted((r["unit_price"], r["count"]) for r in maples) == [(40, 1), (100, 3)], "an override makes its own row"
    by_name = {r["name"]: r for r in sched["rows"] if r["name"] != "Field maple"}
    assert sum(r["total_price"] for r in maples) == approx(340)
    assert maples[0]["id_code"] == "AC"
    assert by_name["Lavender"]["count"] == 9 and by_name["Lavender"]["total_price"] == approx(57.6)
    assert by_name["Lavender"]["size"] == "P9" and by_name["Lavender"]["category"] == "perennial"
    assert by_name["Quercus robur"]["style"] is None and by_name["Quercus robur"]["total_price"] == 0
    assert sched["total_count"] == 14
    assert sched["total_price"] == approx(397.6)
    plants = cad.result("report.plants")["rows"]
    assert sorted((r["species"], r["count"]) for r in plants) == [("Acer campestre", 1), ("Acer campestre", 3), ("Lavandula", 9), ("Quercus robur", 1)]
    assert sum(r["total_price"] for r in plants if r["species"] == "Acer campestre") == approx(340)
    assert [r for r in plants if r["species"] == "Quercus robur"] == [{"species": "Quercus robur", "count": 1}], "plants without a style keep the old row format"


def test_schedule_csv_export(cad):
    make_style(cad)
    place(cad, mode="poly", style="Field maple", points=[[0, 0], [1, 1]])
    text = cad.result("report.plant_schedule", {"format": "csv"})["csv"]
    rows = list(csv.reader(io.StringIO(text)))
    assert rows[0][:2] == ["Name", "ID"]
    assert rows[1][0] == "Field maple" and rows[1][10] == "2" and rows[1][12] == "200.00"
    assert rows[-1][0] == "Total" and rows[-1][10] == "2"


def test_schedule_is_live(cad):
    sid = make_style(cad)
    place(cad, mode="single", style=sid, at=[0, 0])
    assert cad.result("report.plant_schedule")["total_price"] == 100
    cad.result("plant_style.update", {"id": sid, "price": 250})
    assert cad.result("report.plant_schedule")["total_price"] == 250
    pid = plant_objects(cad)[0]["id"]
    cad.execute({"op": "delete_objects", "ids": [pid]})
    assert cad.result("report.plant_schedule")["total_count"] == 0


# --- CSV catalog ----------------------------------------------------------------------


def test_sample_catalog_imports_and_round_trips(cad, make_serve):
    sample = cad.result("plant_style.sample_csv")["csv"]
    r = cad.result("plant_style.import_csv", {"csv": sample})
    assert r["created_count"] == 8 and r["warnings"] == []
    first = cad.result("plant_style.get", {"id": "Field maple"})
    assert first["spread"] == 7000 and first["height"] == 12000  # the sample uses meters
    assert first["category"] == "tree" and first["price"] == 185
    assert first["custom"] == {"Water need": "low"}

    exported = cad.result("plant_style.export_csv")["csv"]
    other = make_serve()
    other.result("plant_style.import_csv", {"csv": exported})

    def strip(styles):
        return [{k: v for k, v in s.items() if k != "id"} for s in styles]

    assert strip(other.result("plant_style.list")["styles"]) == strip(cad.result("plant_style.list")["styles"])


def test_import_maps_columns_by_header_with_units_and_reports_bad_rows(cad):
    text = "Plant;Latin;Width (cm);Height (cm);Kind;Cost\nBox;Buxus;120;100;shrub;12,5\n;;1;1;;\nBad;Bad;-4;1;;\n"
    r = cad.result("plant_style.import_csv", {"csv": text, "mapping": {"name": "Plant", "category": "Kind"}})
    assert r["created_count"] == 1
    assert len(r["warnings"]) == 2
    box = cad.result("plant_style.get", {"id": "Box"})
    assert (box["botanical_name"], box["spread"], box["height"], box["price"]) == ("Buxus", 1200, 1000, 12.5)


def test_import_skips_or_updates_existing_names_and_is_one_undo_step(cad):
    make_style(cad)
    text = "name,spread (m)\nField maple,9\nNew,2\n"
    r = cad.result("plant_style.import_csv", {"csv": text})
    assert r["skipped"] == ["Field maple"] and r["created_count"] == 1
    r = cad.result("plant_style.import_csv", {"csv": text, "on_duplicate": "update"})
    assert len(r["updated"]) == 2  # both names exist now
    updated = cad.result("plant_style.get", {"id": "Field maple"})
    # Only the columns of the file change. The other fields stay.
    assert updated["spread"] == 9000 and updated["botanical_name"] == "Acer campestre" and updated["price"] == 100
    assert cad.result("undo")["done"]
    assert cad.result("plant_style.get", {"id": "Field maple"})["spread"] == 7000
    assert cad.result("undo")["done"]
    assert [s["name"] for s in cad.result("plant_style.list")["styles"]] == ["Field maple"]


def test_import_errors(cad):
    assert cad.error("plant_style.import_csv", {"csv": ""})["code"] == -32602
    assert cad.error("plant_style.import_csv", {"csv": "price\n3\n"})["code"] == -32602
    assert cad.error("plant_style.import_csv", {"csv": "name\nA\n", "unit": "ft"})["code"] == -32602


# --- Existing trees -------------------------------------------------------------------


def tree(**fields):
    shape = {"kind": "existing_tree", "at": [0, 0], "species": "Quercus robur", "dbh": 500, "spread": 9000, "height": 15000}
    shape.update(fields)
    return {"op": "add_object", "shape": shape}


def test_existing_tree_has_a_root_protection_zone_from_dbh(cad):
    cad.execute(tree())
    cad.execute(tree(at=[30000, 0], dbh=300, rpz_factor=10))
    rows = cad.result("report.existing_trees")["rows"]
    assert [r["rpz_radius"] for r in rows] == [6000, 3000]
    assert rows[0]["condition"] == "fair" and rows[0]["action"] == "retain"


def test_existing_tree_display_shows_the_zone_and_dashes_removed_trees(cad):
    cad.execute(tree(action="remove", condition="poor"))
    svg = cad.result("render.svg")["svg"]
    assert 'data-kind="existing_tree"' in svg and 'data-action="remove"' in svg and 'r="6000"' in svg
    assert "stroke-dasharray" in svg
    kept = cad.execute(tree(at=[40000, 0]))[0]
    assert kept


def test_existing_tree_validation(cad):
    for bad in ({"dbh": 0}, {"spread": -1}, {"rpz_factor": -2}, {"dbh": 1e12}):
        assert cad.error("execute", {"command": tree(**bad)})["code"] in (1, -32602)
    assert cad.error("execute", {"command": tree(action="chop")})["code"] == -32602
    assert cad.result("report.existing_trees")["rows"] == []


def test_existing_trees_are_not_in_the_plant_schedule(cad):
    cad.execute(tree())
    assert cad.result("report.plant_schedule")["total_count"] == 0


# --- 3D and files ---------------------------------------------------------------------


@pytest.mark.parametrize("form", ["tree", "columnar", "conical", "spreading", "shrub_mound"])
def test_every_plant_form_renders_in_3d(cad, form):
    sid = make_style(cad, form=form)
    place(cad, mode="single", style=sid, at=[0, 0])
    cad.execute(tree(at=[12000, 0]))
    png = cad.result("render.png", {"width": 160, "height": 120})
    assert png["width"] == 160 and png["png_base64"]


def test_old_documents_load_and_plants_keep_their_own_fields(cad):
    doc = {
        "format_version": 1,
        "layers": [{"id": 1, "name": "Layer 1", "visible": True, "locked": False, "elevation": 0.0, "scale": 100.0}],
        "classes": [{"id": 0, "name": "None", "visible": True, "attrs": {}}],
        "objects": [{"id": 5, "layer": 1, "class": 0,
                     "shape": {"kind": "plant", "at": [1.0, 2.0], "species": "Quercus robur", "spread": 6000.0, "height": 12000.0}}],
        "active_layer": 1,
        "active_class": 0,
        "next_id": 6,
    }
    cad.result("document.load", {"document": doc})
    obj = cad.result("objects.get", {"id": 5})
    assert (obj["shape"]["species"], obj["shape"]["spread"], obj["shape"]["height"]) == ("Quercus robur", 6000, 12000)
    rows = cad.result("report.plant_schedule")["rows"]
    assert [(r["name"], r["count"], r["style"]) for r in rows] == [("Quercus robur", 1, None)]
    assert "resources" not in cad.doc()
    # Such a plant can join a style later.
    sid = make_style(cad)
    cad.result("plants.set_style", {"ids": [5], "style": sid})
    assert cad.result("objects.get", {"id": 5})["shape"]["spread"] == 7000


def test_styles_survive_save_and_open(make_serve, tmp_path):
    first = make_serve()
    sid = make_style(first)
    place(first, mode="poly", style=sid, points=[[0, 0], [100, 100]], overrides={"price": 7})
    path = tmp_path / "garden.cdk.json"
    first.result("file.save", {"path": str(path)})
    second = make_serve(path)
    assert [s["name"] for s in second.result("plant_style.list")["styles"]] == ["Field maple"]
    sched = second.result("report.plant_schedule")
    assert sched["total_count"] == 2 and sched["total_price"] == 14
    # IDs stay unique after loading.
    new_id = make_style(second, "Another")
    assert new_id not in {sid, *[o["id"] for o in plant_objects(second)]}


def test_mcp_offers_the_plant_tools(mcp):
    created = mcp.tool_json("plant_style_create", {"name": "Box", "spread": 1000, "height": 800})
    placed = mcp.tool_json("plants_place", {"mode": "single", "style": created["id"], "at": [0, 0]})
    assert placed["count"] == 1
    assert mcp.tool_json("report_plant_schedule")["total_count"] == 1


def test_schedule_rows_split_by_size_container_and_root(cad):
    sid = make_style(cad)
    place(cad, mode="poly", style=sid, points=[[0, 0]], overrides={"size": "20-25"})
    place(cad, mode="poly", style=sid, points=[[1, 1]], overrides={"container": "C30"})
    place(cad, mode="poly", style=sid, points=[[2, 2], [3, 3]], overrides={"root": "bare root"})
    place(cad, mode="poly", style=sid, points=[[4, 4]])
    rows = cad.result("report.plant_schedule")["rows"]
    assert len(rows) == 4 and sum(r["count"] for r in rows) == 5


def test_set_data_cannot_change_the_plant_link(cad):
    sid = make_style(cad)
    pid = place(cad, mode="single", style=sid, at=[0, 0])["created"][0]
    err = cad.error("execute", {"command": {"op": "set_data", "id": pid, "set": {"plant": {"style": 999}}}})
    assert "reserved" in err["message"]
    assert cad.error("execute", {"command": {"op": "set_data", "id": pid, "remove": ["plant"]}})["code"] == 1
    assert cad.result("objects.get", {"id": pid})["data"]["plant"]["style"] == sid
    cad.execute({"op": "set_data", "id": pid, "set": {"note": "ok"}})


def test_huge_counts_are_refused_quickly(cad):
    sid = make_style(cad)
    err = cad.error("plants.place", {"mode": "path", "style": sid, "path": [[0, 0], [1000, 0]], "count": 400000000})
    assert err["code"] == -32602 and "too many" in err["message"]
    err = cad.error("plants.place", {"mode": "fill", "style": sid, "boundary": SQUARE, "count": 400000000})
    assert "too many" in err["message"]


def test_fill_with_holes_and_spacing_from_spread(cad):
    sid = make_style(cad, spread=500)
    hole = [[1000, 1000], [3000, 1000], [3000, 3000], [1000, 3000]]
    full = place(cad, mode="fill", style=sid, boundary=SQUARE, spacing="spread", pattern="rectangular")["count"]
    holed = place(cad, mode="fill", style=sid, boundary=SQUARE, holes=[hole], spacing="spread", pattern="rectangular")["count"]
    assert full == 81 and full - holed == 9
    for o in plant_objects(cad)[full:]:
        x, y = o["shape"]["at"]
        assert not (1000 < x < 3000 and 1000 < y < 3000)


def test_root_protection_radius_is_capped_at_15_m(cad):
    cad.execute(tree(dbh=1500))
    cad.execute(tree(at=[50000, 0], dbh=1500, rpz_cap=20000))
    rows = cad.result("report.existing_trees")["rows"]
    assert [r["rpz_radius"] for r in rows] == [15000, 18000]


def test_catalog_numbers_with_both_separators_and_ambiguous_ones(cad):
    r = cad.result("plant_style.import_csv", {"csv": "name;price;spread\nA;1.234,50;1,234\n", "unit": "m"})
    assert r["created_count"] == 1
    a = cad.result("plant_style.get", {"id": "A"})
    assert a["price"] == 1234.5 and a["spread"] == 1234
    assert any("1,234" in w for w in r["warnings"])


def test_unknown_resource_types_survive_a_load_and_save(cad):
    doc = cad.doc()
    doc["resources"] = [{"id": 90, "name": "Hatch", "body": {"type": "hatch", "angle": 45}}]
    cad.result("document.load", {"document": doc})
    assert cad.doc()["resources"] == doc["resources"]
    assert cad.result("plant_style.list")["styles"] == []
    sid = make_style(cad)
    assert sid != 90
    assert cad.doc()["resources"][0] == doc["resources"][0]


# --- Locked fields, variation, area quantity, protection zone ------------------------------


def test_locked_style_fields_refuse_overrides_and_snap_back(cad):
    sid = make_style(cad, locked=["price", "spread"])
    err = cad.error("plants.place", {"mode": "single", "style": sid, "at": [0, 0], "overrides": {"price": 1}})
    assert "locked" in err["message"]
    pid = place(cad, mode="single", style=sid, at=[0, 0], overrides={"height": 5000})["created"][0]
    shape = cad.result("objects.get", {"id": pid})["shape"]
    cad.execute({"op": "modify_object", "id": pid, "shape": {**shape, "spread": 99, "height": 6000}})
    obj = cad.result("objects.get", {"id": pid})
    assert obj["shape"]["spread"] == 7000 and obj["shape"]["height"] == 6000
    assert obj["data"]["plant"]["overrides"] == {"height": 6000}
    assert cad.error("plants.set_style", {"ids": [pid], "overrides": {"price": 2}})["code"] == 1
    assert cad.error("plant_style.update", {"id": sid, "locked": ["colour"]})["code"] == -32602


def test_size_and_rotation_variation_is_seeded(cad):
    sid = make_style(cad)
    pts = [[i * 9000, 0] for i in range(12)]
    variation = {"seed": 11, "size": 0.25, "rotation": True}
    a = place(cad, mode="poly", style=sid, points=pts, variation=variation)["created"]
    b = place(cad, mode="poly", style=sid, points=pts, variation=variation)["created"]
    c = place(cad, mode="poly", style=sid, points=pts, variation={**variation, "seed": 12})["created"]

    def look(ids):
        objs = [cad.result("objects.get", {"id": i}) for i in ids]
        return [(o["shape"]["spread"], o["data"]["rotation"]) for o in objs]

    assert look(a) == look(b) and look(a) != look(c)
    assert all(7000 * 0.75 <= s <= 7000 * 1.25 and 0 <= r < 6.2832 for s, r in look(a))
    assert len({s for s, _ in look(a)}) > 6
    assert len(cad.result("report.plant_schedule")["rows"]) == 1, "variation keeps one row"
    assert cad.error("plants.place", {"mode": "single", "style": sid, "at": [0, 0], "variation": {"size": 2}})["code"] == 1
    assert 'data-graphic="cloud"' in cad.result("render.svg")["svg"]


def test_groundcover_quantity_by_area(cad):
    gc = make_style(cad, "Thyme", category="groundcover", spread=1000, price=12, quantity="area", graphic="clover", form="shrub_mound")
    place(cad, mode="grid", style=gc, min=[0, 0], max=[1000, 1000], spacing=1000)
    sched = cad.result("report.plant_schedule", {"format": "csv"})
    row = sched["rows"][0]
    assert row["quantity"] == "area" and row["count"] == 4
    assert row["area_m2"] == approx(4 * 3.14159265 / 4)
    assert row["total_price"] == approx(row["area_m2"] * 12)
    assert sched["total_area_m2"] == approx(row["area_m2"])
    assert "Area (m2)" in sched["csv"]


def test_tree_plants_can_show_a_protection_zone(cad):
    oak = make_style(cad, "Oak", trunk_diameter=400, protection_zone=True)
    pid = place(cad, mode="single", style=oak, at=[0, 0])["created"][0]
    svg = cad.result("render.svg")["svg"]
    assert 'data-kind="tree_protection_zone"' in svg and 'r="4800"' in svg
    cad.result("plants.set_style", {"ids": [pid], "overrides": {"trunk_diameter": 1500}})
    assert 'r="15000"' in cad.result("render.svg")["svg"], "same 15 m cap as existing trees"
    shrub = make_style(cad, "Box", category="shrub", trunk_diameter=400, protection_zone=True)
    place(cad, mode="single", style=shrub, at=[50000, 0])
    assert cad.result("render.svg")["svg"].count('data-kind="tree_protection_zone"') == 1
