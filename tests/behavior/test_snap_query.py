"""Scenario: an agent finds objects by criteria, asks where a point snaps to, and greys out layers."""

import pytest

from helpers import line_cmd, plant_cmd, rect_cmd, uid


def circle_cmd(cx, cy, r, **extra):
    return {"op": "add_object", "shape": {"kind": "circle", "center": [cx, cy], "radius": r}, **extra}


# --- objects.query -------------------------------------------------------------------------------


def test_query_by_kind_and_area(cad):
    r1 = cad.execute(rect_cmd(0, 0, 1000, 1000))[0]  # 1 m2
    r2 = cad.execute(rect_cmd(0, 0, 5000, 5000))[0]  # 25 m2
    cad.execute(plant_cmd(0, 0))
    out = cad.result("objects.query", {"criteria": 'Kind = "rect"'})
    assert out["ids"] == [r1, r2] and out["count"] == 2 and not out["truncated"]
    big = cad.result("objects.query", {"criteria": 'Kind = "rect" AND Area > 10'})
    assert big["ids"] == [r2]
    everything = cad.result("objects.query", {})
    assert everything["count"] == 3


def test_query_returns_columns_and_can_select(cad):
    cad.execute(plant_cmd(0, 0, species="Oak"))
    cad.execute(plant_cmd(10, 0, species="Birch"))
    out = cad.result("objects.query", {"criteria": 'Kind = "plant"', "columns": ["Kind", "Name"], "select": True})
    assert [r["values"][0] for r in out["rows"]] == ["plant", "plant"]
    assert cad.result("selection.get")["ids"] == out["ids"]


def test_query_by_layer_and_class_names_and_filters(cad):
    layer = cad.execute({"op": "add_layer", "name": "Site", "elevation": 0})[0]
    cad.execute(rect_cmd(0, 0, 10, 10))
    on_site = cad.execute(rect_cmd(0, 0, 20, 20, layer=layer))[0]
    out = cad.result("objects.query", {"criteria": 'Layer = "Site"'})
    assert out["ids"] == [on_site]
    # The same filters as objects.list work too.
    assert cad.result("objects.query", {"layer": layer})["ids"] == [on_site]
    assert cad.result("objects.query", {"kind": "circle"})["count"] == 0


def test_query_limit_and_errors(cad):
    for i in range(5):
        cad.execute(rect_cmd(i, 0, i + 1, 1))
    out = cad.result("objects.query", {"limit": 2})
    assert out["count"] == 5 and len(out["ids"]) == 2 and out["truncated"]
    assert cad.error("objects.query", {"criteria": "Kind ="})["code"] == -32602
    assert cad.error("objects.query", {"criteria": "Kind = 1", "columns": ["((("]})["code"] == -32602
    # A name that does not exist leaves the object out and is counted.
    out = cad.result("objects.query", {"criteria": "NoSuchProperty > 1"})
    assert out["count"] == 0 and out["skipped"] == 5


def test_query_uses_record_fields(cad):
    fmt = cad.result("record_format.create", {"name": "Soil", "fields": [{"name": "Depth", "type": "dimension", "default": 300}]})
    oid = cad.execute(rect_cmd(0, 0, 10, 10))[0]
    cad.execute(rect_cmd(20, 0, 30, 10))
    cad.result("record.attach", {"ids": [oid], "record": fmt["id"], "values": {"Depth": 500}})
    out = cad.result("objects.query", {"criteria": "'Soil'.'Depth' > 400"})
    assert out["ids"] == [oid]


# --- snap ----------------------------------------------------------------------------------------


def test_snap_defaults_and_set(cad):
    s = cad.result("snap.get")
    assert s["set"]["grid"] and s["set"]["object_points"] and not s["set"]["angle"]
    s = cad.result("snap.set", {"set": {"angle": True, "distance": True}, "angle_step_deg": 45, "distance_step": 100})
    assert s["set"]["angle"] and s["set"]["distance"] and s["angle_step_deg"] == pytest.approx(45)
    assert cad.error("snap.set", {"set": {"nope": True}})["code"] == -32602
    assert cad.error("snap.set", {"angle_step_deg": 0})["code"] == -32602
    assert cad.error("snap.set", {"set": {"grid": "yes"}})["code"] == -32602


def test_snap_to_object_points(cad):
    cad.execute(line_cmd(0, 0, 1000, 0))
    r = cad.result("snap.query", {"point": [3, 4], "tolerance": 10})
    assert (r["snapped"], r["kind"], r["point"], r["cue"]) == (True, "endpoint", [0, 0], "Endpoint")
    r = cad.result("snap.query", {"point": [502, 4], "tolerance": 10})
    assert (r["kind"], r["point"], r["cue"]) == ("midpoint", [500, 0], "Midpoint")
    # Far away: the grid.
    r = cad.result("snap.query", {"point": [1234, 777], "tolerance": 10, "grid_step": 100})
    assert (r["kind"], r["point"]) == ("grid", [1200, 800])
    # The caller can switch categories for one call.
    r = cad.result("snap.query", {"point": [502, 4], "tolerance": 10, "set": {"midpoints": False, "grid": False}})
    assert r["snapped"] is False
    # Excluded objects are not snapped to.
    oid = cad.result("objects.list")[0]["id"]
    r = cad.result("snap.query", {"point": [3, 4], "tolerance": 10, "exclude": [oid], "set": {"grid": False}})
    assert r["snapped"] is False


def test_snap_to_intersections_and_circle_quadrants(cad):
    cad.execute(line_cmd(0, 0, 1000, 1000))
    cad.execute(line_cmd(0, 200, 1000, 0))
    # The lines cross at (166.67, 166.67), which is not an end or a middle of either.
    r = cad.result("snap.query", {"point": [175, 160], "tolerance": 20})
    assert r["kind"] == "intersection" and r["point"] == pytest.approx([1000 / 6, 1000 / 6])
    cad.execute(circle_cmd(3000, 0, 400))
    r = cad.result("snap.query", {"point": [3400, 5], "tolerance": 20})
    assert r["kind"] == "quadrant" and r["point"] == [3400, 0]


def test_snap_angle_and_distance(cad):
    cad.result("snap.set", {"set": {"angle": True, "distance": True, "grid": False}, "angle_step_deg": 45, "distance_step": 100})
    r = cad.result("snap.query", {"point": [214, 212], "anchor": [0, 0], "tolerance": 10})
    assert r["kind"] == "angle" and "45" in r["cue"] and "length" in r["cue"]
    assert r["point"] == pytest.approx([300 / 2 ** 0.5, 300 / 2 ** 0.5])
    assert r["guides"]


def test_snap_perpendicular_and_tangent(cad):
    cad.execute(line_cmd(0, 0, 1000, 0))
    r = cad.result("snap.query", {"point": [405, 3], "anchor": [400, 600], "tolerance": 10, "set": {"perpendicular": True, "grid": False, "object_points": False, "midpoints": False}})
    assert r["kind"] == "perpendicular" and r["point"] == [400, 0]
    cad.execute(circle_cmd(5000, 0, 300))
    # From (5000, 1000) the tangent points of the circle are at 17.46 and 162.54 degrees.
    x, y = 5000 + 300 * 0.9539392, 300 * 0.3
    r = cad.result("snap.query", {"point": [x + 5, y + 5], "anchor": [5000, 1000], "tolerance": 20, "set": {"tangent": True, "grid": False, "object_points": False}})
    assert r["kind"] == "tangent" and r["point"] == pytest.approx([x, y], abs=0.5)


def test_smart_points_give_alignment_lines(cad):
    cad.result("snap.smart", {"action": "acquire_point", "point": [100, 200]})
    cad.result("snap.smart", {"action": "acquire_point", "point": [400, 50]})
    r = cad.result("snap.query", {"point": [402, 198], "tolerance": 10, "set": {"grid": False}})
    assert (r["kind"], r["point"]) == ("smart_point", [400, 200])
    assert len(r["guides"]) == 2
    # Four points: the oldest goes (up to three are kept).
    for p in ([1, 1], [2, 2], [3, 3]):
        s = cad.result("snap.smart", {"action": "acquire_point", "point": p})
    assert len(s["smart"]["points"]) == 3
    cad.result("snap.smart", {"action": "clear"})
    assert cad.result("snap.get")["smart"] == {"points": [], "edges": []}
    assert cad.error("snap.smart", {"action": "dance"})["code"] == -32602
    assert cad.error("snap.smart", {"action": "acquire_edge", "a": [0, 0], "b": [0, 0]})["code"] == -32602


def test_smart_acquire_at_an_object_point(cad):
    cad.execute(line_cmd(0, 0, 1000, 0))
    s = cad.result("snap.smart", {"action": "acquire_at", "point": [3, 3], "tolerance": 10})
    assert s["smart"]["points"] == [[0, 0]]
    assert cad.error("snap.smart", {"action": "acquire_at", "point": [5000, 5000]})["code"] == 2
    s = cad.result("snap.smart", {"action": "acquire_at", "point": [300, 4], "edges": True, "tolerance": 10})
    assert len(s["smart"]["edges"]) == 1


# --- visibility modes (grey) and snapping across layers --------------------------------------------


def test_grey_layers_cannot_be_snapped_to_unless_allowed(cad):
    layer = cad.execute({"op": "add_layer", "name": "Survey", "elevation": 0})[0]
    cad.execute(line_cmd(0, 0, 1000, 0, layer=layer))
    near = {"point": [3, 3], "tolerance": 10, "set": {"grid": False}}
    assert cad.result("snap.query", near)["kind"] == "endpoint"
    v = cad.result("visibility.set", {"mode": "grey", "layers": [layer]})
    assert {l["id"]: l["mode"] for l in v["layers"]}[layer] == "grey"
    assert cad.result("snap.query", near)["snapped"] is False
    cad.result("snap.set", {"grey": True})
    assert cad.result("snap.query", near)["kind"] == "endpoint"
    cad.result("snap.set", {"grey": False})
    # Not across layers: only the active layer counts.
    cad.result("visibility.set", {"mode": "visible", "layers": [layer]})
    cad.result("snap.set", {"other_layers": False})
    assert cad.result("snap.query", near)["snapped"] is False
    cad.execute({"op": "set_active", "layer": layer})
    assert cad.result("snap.query", near)["kind"] == "endpoint"


def test_grey_classes_and_undo(cad):
    cls = cad.execute({"op": "add_class", "name": "Existing"})[0]
    cad.execute({**rect_cmd(0, 0, 10, 10), "class": cls})
    cad.result("visibility.set", {"mode": "grey", "classes": [cls]})
    assert cad.result("document.summary")["grey"] == {"classes": [cls]}
    modes = {c["id"]: c["mode"] for c in cad.result("visibility.get")["classes"]}
    assert modes[cls] == "grey"
    cad.result("visibility.set", {"mode": "invisible", "classes": [cls]})
    assert {c["id"]: c["mode"] for c in cad.result("visibility.get")["classes"]}[cls] == "invisible"
    assert "grey" not in cad.result("document.summary") or cad.result("document.summary")["grey"] == {}
    cad.result("undo")
    assert {c["id"]: c["mode"] for c in cad.result("visibility.get")["classes"]}[cls] == "grey"
    assert cad.error("visibility.set", {"mode": "pink", "classes": [cls]})["code"] == -32602
    assert cad.error("visibility.set", {"mode": "grey"})["code"] == 3
    assert cad.error("visibility.set", {"mode": "grey", "layers": [uid(999)]})["code"] == 2


def test_grey_layers_stay_in_exports_and_the_file(cad, tmp_path):
    layer = cad.execute({"op": "add_layer", "name": "Survey", "elevation": 0})[0]
    cad.execute(rect_cmd(0, 0, 10, 10, layer=layer))
    cad.result("visibility.set", {"mode": "grey", "layers": [layer]})
    doc = cad.doc()
    assert doc["drawing"]["grey"]["layers"] == [layer]
    cad.result("document.load", {"document": doc})
    assert cad.result("visibility.get")["layers"][-1]["mode"] == "grey"
