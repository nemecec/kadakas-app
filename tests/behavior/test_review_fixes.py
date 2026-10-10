from helpers import uid
"""Regression tests for the review of the editing, snapping and schema work."""

U_SHAPE = [[0, 0], [3000, 0], [3000, 2000], [2200, 2000], [2200, 600], [800, 600], [800, 2000], [0, 2000]]


def polygon_area(shape):
    pts = [p if isinstance(p, list) else p["p"] for p in shape["points"]]
    n = len(pts)
    return abs(sum(pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1] for i in range(n))) / 2


def poly(cad, pts, closed=True):
    return cad.execute({"op": "add_object", "shape": {"kind": "polyline", "points": pts, "closed": closed}})[0]


def test_an_inward_offset_that_would_cross_itself_is_cleaned_or_refused(cad):
    u = poly(cad, U_SHAPE)
    src = polygon_area(cad.result("objects.get", {"id": u})["shape"])
    out = cad.result("edit.offset", {"id": u, "distance": 250, "side": "inside"})
    piece = cad.result("objects.get", {"id": out["created"][0]})["shape"]
    assert 0 < polygon_area(piece) < src
    for d in (600, 5000):
        err = cad.error("edit.offset", {"id": u, "distance": d, "side": "inside"})
        assert err["code"] == 3 and "collapses" in err["message"], err


def test_an_outward_offset_contains_the_source(cad):
    u = poly(cad, U_SHAPE)
    out = cad.result("edit.offset", {"id": u, "distance": 800, "side": "outside"})
    assert out["created"]
    big = cad.result("objects.get", {"id": out["created"][0]})["shape"]
    assert polygon_area(big) > polygon_area(cad.result("objects.get", {"id": u})["shape"])


def test_one_command_cannot_create_more_than_100000_objects(cad):
    ids = [
        cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [i, 0], "radius": 0.4}})[0]
        for i in range(11)
    ]
    err = cad.error("edit.array", {"ids": ids, "shape": "linear", "count": 10000, "offset": [0, 5]})
    assert err["code"] == 3 and "110000" in err["message"]
    assert cad.result("document.summary")["object_count"] == 11


def test_grey_objects_cannot_be_selected_or_edited(cad):
    layer = cad.execute({"op": "add_layer", "name": "Survey", "elevation": 0})[0]
    a = cad.execute({"op": "add_object", "layer": layer, "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}})[0]
    b = cad.execute({"op": "add_object", "shape": {"kind": "rect", "min": [20, 0], "max": [30, 10]}})[0]
    cad.result("visibility.set", {"mode": "grey", "layers": [layer]})
    q = cad.result("objects.query", {"select": True})
    assert q["ids"] == [a, b] and q["grey_not_selected"] == 1
    assert cad.result("selection.get")["ids"] == [b]
    assert cad.error("selection.set", {"ids": [a]})["code"] == 3
    for cmd in (
        {"op": "move_objects", "ids": [a], "delta": [1, 1]},
        {"op": "delete_objects", "ids": [a]},
        {"op": "set_active", "layer": layer},
    ):
        err = cad.rpc("execute", {"command": cmd})["error"]
        assert err["code"] == 3 and "grey" in err["message"], err
    err = cad.error("visibility.set", {"mode": "grey", "layers": [uid(1)]})
    assert "active layer" in err["message"]
    cad.result("visibility.set", {"mode": "visible", "layers": [layer]})
    cad.execute({"op": "move_objects", "ids": [a], "delta": [1, 1]})


def test_pieces_keep_the_records_of_the_object(cad):
    fmt = cad.result(
        "record_format.create",
        {"name": "Pset", "fields": [{"name": "Depth", "type": "dimension", "default": 300}]},
    )
    t = cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}})[0]
    cad.result("record.attach", {"ids": [t], "record": fmt["id"], "values": {"Depth": 250}})
    out = cad.result("edit.split", {"id": t, "at": [[400, 0]]})
    rec = cad.result("record.get", {"id": out["created"][0]})
    assert "Pset" in str(rec) and "250" in str(rec)
    off = cad.result("edit.offset", {"id": t, "distance": 10})
    assert "250" in str(cad.result("record.get", {"id": off["created"][0]}))


def test_the_schema_describes_shapes_field_by_field(cad):
    desc = cad.result("describe", {"schema": True})
    assert "line" in desc["schemas"]["exact_shape_kinds"]
    assert "massing" in desc["schemas"]["structural_only_shape_kinds"]
    assert "structural_only_commands" in desc["schemas"]
    schema = desc["command_schema"]
    assert len(schema["$defs"]["Shape"]["anyOf"]) == len(desc["schemas"]["exact_shape_kinds"]) + 1
    assert schema["$defs"]["ObjectId"]["type"] == "string"
    assert schema["$defs"]["ObjectId"]["pattern"].startswith("^[0-9a-fA-F]{8}-")
