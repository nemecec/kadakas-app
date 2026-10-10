"""Scenario: an agent edits 2D geometry: rotate, scale, mirror, arrays, offset, fillet, chamfer, trim,
extend, split and surface operations. Every edit is one undo step."""

import math

import pytest

from helpers import line_cmd, rect_cmd, strip_next_id, uid


def shape_of(cad, oid):
    return cad.result("objects.get", {"id": oid})["shape"]


def circle_cmd(cx, cy, r):
    return {"op": "add_object", "shape": {"kind": "circle", "center": [cx, cy], "radius": r}}


def poly_cmd(points, closed=True):
    return {"op": "add_object", "shape": {"kind": "polyline", "points": points, "closed": closed}}


def areas(cad):
    out = {}
    for o in cad.result("objects.list"):
        s = o["shape"]
        if s["kind"] == "rect":
            (x0, y0), (x1, y1) = s["min"], s["max"]
            out[o["id"]] = abs((x1 - x0) * (y1 - y0))
        elif s["kind"] == "circle":
            out[o["id"]] = math.pi * s["radius"] ** 2
        elif s["kind"] == "polyline":
            pts = [p if isinstance(p, list) else p["p"] for p in s["points"]]
            n = len(pts)
            out[o["id"]] = abs(sum(pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1] for i in range(n))) / 2
    return out


def test_scale_a_circle_and_undo(cad):
    c = cad.execute(circle_cmd(1000, 0, 500))[0]
    before = strip_next_id(cad.doc())
    cad.result("edit.scale", {"ids": [c], "center": [0, 0], "sx": 2})
    s = shape_of(cad, c)
    assert s["center"] == [2000, 0] and s["radius"] == 1000
    cad.result("undo")
    assert strip_next_id(cad.doc()) == before


def test_a_circle_cannot_be_stretched(cad):
    c = cad.execute(circle_cmd(0, 0, 500))[0]
    err = cad.error("edit.scale", {"ids": [c], "center": [0, 0], "sx": 2, "sy": 3})
    assert err["code"] == 3 and "same scale" in err["message"]
    assert cad.error("edit.scale", {"ids": [c], "center": [0, 0], "sx": 0})["code"] == 3


def test_a_rectangle_stretches_and_stays_a_rectangle(cad):
    r = cad.execute(rect_cmd(0, 0, 1000, 500))[0]
    cad.result("edit.scale", {"ids": [r], "center": [0, 0], "sx": 2, "sy": 3})
    assert shape_of(cad, r) == {"kind": "rect", "min": [0, 0], "max": [2000, 1500]}


def test_mirror_move_and_copy(cad):
    r = cad.execute(rect_cmd(100, 0, 300, 100))[0]
    out = cad.result("edit.mirror", {"ids": [r], "a": [0, 0], "b": [0, 1], "copy": True})
    assert len(out["created"]) == 1
    assert shape_of(cad, out["created"][0]) == {"kind": "rect", "min": [-300, 0], "max": [-100, 100]}
    assert shape_of(cad, r)["min"] == [100, 0]
    cad.result("edit.mirror", {"ids": [r], "a": [0, 0], "b": [0, 1]})
    assert shape_of(cad, r)["min"] == [-300, 0]
    assert cad.error("edit.mirror", {"ids": [r], "a": [1, 1], "b": [1, 1]})["code"] == 3


def test_rotate_with_copy_uses_the_selection(cad):
    r = cad.execute(rect_cmd(0, 0, 100, 100))[0]
    cad.result("selection.set", {"ids": [r]})
    out = cad.result("edit.rotate", {"center": [0, 0], "angle": math.pi / 2, "copy": True})
    assert cad.result("selection.get")["ids"] == out["created"]
    assert len(cad.result("objects.list")) == 2
    assert cad.error("edit.rotate", {"ids": [], "center": [0, 0], "angle": 1})["code"] == -32602


def test_linear_array(cad):
    c = cad.execute(circle_cmd(0, 0, 10))[0]
    out = cad.result("edit.array", {"ids": [c], "shape": "linear", "count": 3, "offset": [100, 50]})
    assert len(out["created"]) == 3
    assert shape_of(cad, out["created"][2])["center"] == [300, 150]
    cad.result("undo")
    assert len(cad.result("objects.list")) == 1


def test_rectangular_array_has_rows_times_columns_places(cad):
    c = cad.execute(circle_cmd(0, 0, 10))[0]
    out = cad.result("edit.array", {"ids": [c], "shape": "rectangular", "columns": 3, "rows": 2, "column_spacing": 100, "row_spacing": 200})
    assert len(out["created"]) == 5
    assert len(cad.result("objects.list")) == 6
    centers = sorted(tuple(o["shape"]["center"]) for o in cad.result("objects.list"))
    assert centers == [(0, 0), (0, 200), (100, 0), (100, 200), (200, 0), (200, 200)]


def test_circular_array_keeps_the_radius(cad):
    c = cad.execute(circle_cmd(1000, 0, 10))[0]
    out = cad.result("edit.array", {"ids": [c], "shape": "circular", "count": 3, "center": [0, 0]})
    for oid in out["created"]:
        x, y = shape_of(cad, oid)["center"]
        assert math.hypot(x, y) == pytest.approx(1000)
    # Four places over a full turn: the third copy is on the other side.
    x, y = shape_of(cad, out["created"][1])["center"]
    assert (x, y) == pytest.approx((-1000, 0), abs=1e-6)


def test_array_resize_rotate_and_errors(cad):
    r = cad.execute(rect_cmd(0, 0, 10, 10))[0]
    out = cad.result("edit.array", {"ids": [r], "shape": "linear", "count": 2, "offset": [100, 0], "resize": 2})
    s = shape_of(cad, out["created"][1])
    assert (s["max"][0] - s["min"][0]) == pytest.approx(40)
    assert cad.error("edit.array", {"ids": [r], "shape": "linear", "count": 0, "offset": [1, 0]})["code"] == 3
    assert cad.error("edit.array", {"ids": [r], "shape": "linear", "count": 2})["code"] == 3
    assert cad.error("edit.array", {"ids": [r], "shape": "spiral"})["code"] == -32602


def test_offset_a_rectangle_and_a_line(cad):
    r = cad.execute(rect_cmd(0, 0, 1000, 1000))[0]
    out = cad.result("edit.offset", {"id": r, "distance": 100})
    assert shape_of(cad, out["created"][0]) == {"kind": "rect", "min": [-100, -100], "max": [1100, 1100]}
    ln = cad.execute(line_cmd(0, 0, 1000, 0))[0]
    out = cad.result("edit.offset", {"id": ln, "distance": 50, "side": "right"})
    assert shape_of(cad, out["created"][0]) == {"kind": "line", "a": [0, -50], "b": [1000, -50]}
    out = cad.result("edit.offset", {"id": ln, "through": [300, 80], "copy": False})
    assert shape_of(cad, ln)["a"] == [0, 80]
    assert cad.error("edit.offset", {"id": ln})["code"] == -32602
    assert cad.error("edit.offset", {"id": ln, "distance": 5, "side": "up"})["code"] == -32602


def test_offset_a_polyline_with_an_arc_vertex(cad):
    p = cad.execute(poly_cmd([[0, 0], {"p": [1000, 0], "kind": "arc", "radius": 300}, [1000, 1000]], closed=False))[0]
    out = cad.result("edit.offset", {"id": p, "distance": 100, "side": "right"})
    s = shape_of(cad, out["created"][0])
    assert s["kind"] == "polyline"
    assert any(isinstance(v, dict) and v["kind"] == "point_on_arc" for v in s["points"])


def test_fillet_two_lines(cad):
    a = cad.execute(line_cmd(0, 0, 1000, 0))[0]
    b = cad.execute(line_cmd(0, 0, 0, 1000))[0]
    out = cad.result("edit.fillet", {"a": a, "b": b, "pick_a": [800, 0], "pick_b": [0, 800], "radius": 200})
    assert len(out["created"]) == 1
    arc = shape_of(cad, out["created"][0])
    assert arc["kind"] == "arc" and arc["radius"] == 200
    assert shape_of(cad, a)["a"] == pytest.approx([200, 0])
    assert shape_of(cad, b)["a"] == pytest.approx([0, 200])
    cad.result("undo")
    assert shape_of(cad, a)["a"] == [0, 0] and len(cad.result("objects.list")) == 2


def test_fillet_modes(cad):
    a = cad.execute(line_cmd(-500, 0, 1000, 0))[0]
    b = cad.execute(line_cmd(0, -500, 0, 1000))[0]
    args = {"a": a, "b": b, "pick_a": [800, 0], "pick_b": [0, 800], "radius": 100}
    cad.result("edit.fillet", {**args, "mode": "standard"})
    assert shape_of(cad, a)["a"] == [-500, 0]
    cad.result("undo")
    out = cad.result("edit.fillet", {**args, "mode": "split"})
    assert len(out["created"]) == 3  # the arc and the two cut-off parts
    assert cad.error("edit.fillet", {**args, "mode": "round"})["code"] == -32602
    assert cad.error("edit.fillet", {"a": a, "b": b, "radius": 5})["code"] == -32602


def test_round_every_corner_of_a_rectangle(cad):
    p = cad.execute(poly_cmd([[0, 0], [1000, 0], [1000, 1000], [0, 1000]]))[0]
    cad.result("edit.fillet", {"id": p, "all": True, "radius": 100})
    s = shape_of(cad, p)
    assert sum(1 for v in s["points"] if isinstance(v, dict) and v["kind"] == "point_on_arc") == 4
    cad.result("undo")
    assert shape_of(cad, p)["points"][0] == [0, 0]
    assert cad.error("edit.fillet", {"id": p, "all": True, "radius": 600})["code"] == 3


def test_chamfer_forms(cad):
    a = cad.execute(line_cmd(0, 0, 1000, 0))[0]
    b = cad.execute(line_cmd(0, 0, 0, 1000))[0]
    base = {"a": a, "b": b, "pick_a": [800, 0], "pick_b": [0, 800]}
    out = cad.result("edit.chamfer", {**base, "distance": 200, "distance2": 300})
    line = shape_of(cad, out["created"][0])
    assert line["a"] == pytest.approx([200, 0]) and line["b"] == pytest.approx([0, 300])
    cad.result("undo")
    out = cad.result("edit.chamfer", {**base, "length": 200 * math.sqrt(2)})
    assert shape_of(cad, out["created"][0])["a"] == pytest.approx([200, 0])
    cad.result("undo")
    assert cad.error("edit.chamfer", base)["code"] == -32602
    assert cad.error("edit.chamfer", {**base, "distance": 5000})["code"] == 3


def test_trim_removes_the_part_between_crossings(cad):
    t = cad.execute(line_cmd(0, 0, 1000, 0))[0]
    cad.execute(line_cmd(300, -100, 300, 100))
    cad.execute(line_cmd(700, -100, 700, 100))
    out = cad.result("edit.trim", {"id": t, "pick": [500, 0]})
    assert len(out["created"]) == 1
    assert shape_of(cad, t)["b"] == [300, 0]
    assert shape_of(cad, out["created"][0])["a"] == [700, 0]
    cad.result("undo")
    assert shape_of(cad, t)["b"] == [1000, 0]
    # Only the listed cutters count.
    cutters = [o["id"] for o in cad.result("objects.list") if o["id"] != t][:1]
    cad.result("edit.trim", {"id": t, "pick": [500, 0], "cutters": cutters})
    # Only the crossing at 300 counts: the part from 300 to the end goes.
    assert shape_of(cad, t)["b"] == [300, 0]
    assert cad.error("edit.trim", {"id": t, "pick": [5000, 0], "cutters": []})["code"] == 3


def test_trim_a_circle_leaves_an_arc(cad):
    c = cad.execute(circle_cmd(0, 0, 500))[0]
    cad.execute(line_cmd(-1000, 0, 1000, 0))
    cad.result("edit.trim", {"id": c, "pick": [0, 500]})
    s = shape_of(cad, c)
    assert s["kind"] == "arc" and s["radius"] == 500


def test_extend_to_a_boundary(cad):
    t = cad.execute(line_cmd(0, 0, 400, 0))[0]
    b = cad.execute(line_cmd(1000, -500, 1000, 500))[0]
    cad.result("edit.extend", {"id": t, "pick": [350, 0], "boundaries": [b]})
    assert shape_of(cad, t)["b"] == [1000, 0]
    assert cad.error("edit.extend", {"id": t, "pick": [10, 0], "boundaries": [b]})["code"] == 3


def test_split_a_line_and_a_rectangle(cad):
    ln = cad.execute(line_cmd(0, 0, 1000, 0))[0]
    out = cad.result("edit.split", {"id": ln, "at": [[400, 30]]})
    assert len(out["created"]) == 1
    assert shape_of(cad, ln)["b"] == [400, 0]
    r = cad.execute(rect_cmd(0, 0, 1000, 1000))[0]
    before = sum(areas(cad).values())
    out = cad.result("edit.split", {"id": r, "line": {"a": [300, -1], "b": [300, 2000]}})
    assert len(out["created"]) == 1
    pieces = {i: a for i, a in areas(cad).items() if i in (r, out["created"][0])}
    assert sum(pieces.values()) == pytest.approx(1000 * 1000)
    assert sorted(round(v) for v in pieces.values()) == [300000, 700000]
    assert before > 0
    assert cad.error("edit.split", {"id": r})["code"] == -32602
    assert cad.error("edit.split", {"id": r, "line": {"a": [5000, 0], "b": [5000, 1]}})["code"] == 3


def test_surface_add_subtract_clip_intersect_compose(cad):
    def two():
        a = cad.execute(rect_cmd(0, 0, 1000, 1000))[0]
        b = cad.execute(rect_cmd(500, 500, 1500, 1500))[0]
        return a, b

    a, b = two()
    out = cad.result("surface.add", {"ids": [a, b]})
    assert sum(areas(cad).values()) == pytest.approx(1_750_000)
    assert len(cad.result("objects.list")) == 1 and out["created"] == []
    cad.result("undo")
    cad.result("surface.subtract", {"base": a, "cutters": [b]})
    assert sum(areas(cad).values()) == pytest.approx(750_000)
    cad.result("undo")
    cad.result("surface.clip", {"ids": [a, b]})
    assert len(cad.result("objects.list")) == 2  # the clipping shape stays by default
    assert sum(areas(cad).values()) == pytest.approx(750_000 + 1_000_000)
    cad.result("undo")
    cad.result("surface.intersect", {"ids": [a, b]})
    assert sum(areas(cad).values()) == pytest.approx(250_000)
    cad.result("undo")
    out = cad.result("surface.compose", {"ids": [a, b]})
    assert len(out["created"]) == 3
    assert sum(areas(cad).values()) == pytest.approx(1_750_000)


def test_surface_subtract_makes_a_hole(cad):
    a = cad.execute(rect_cmd(0, 0, 1000, 1000))[0]
    b = cad.execute(circle_cmd(500, 500, 200))[0]
    cad.result("surface.subtract", {"base": a, "cutters": [b]})
    objs = cad.result("objects.list")
    assert len(objs) == 1 and objs[0]["shape"]["kind"] == "polyline"
    pts = [p if isinstance(p, list) else p["p"] for p in objs[0]["shape"]["points"]]
    n = len(pts)
    area = abs(sum(pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1] for i in range(n))) / 2
    assert area == pytest.approx(1_000_000 - math.pi * 200 * 200, rel=1e-4)


def test_surface_errors(cad):
    a = cad.execute(rect_cmd(0, 0, 10, 10))[0]
    ln = cad.execute(line_cmd(0, 0, 5, 5))[0]
    assert "line" in cad.error("surface.add", {"ids": [a, ln]})["message"]
    assert cad.error("surface.add", {"ids": [a, uid(999)]})["code"] == 2
    assert cad.error("surface.add", {"ids": [a]})["code"] == 3
    assert cad.error("surface.subtract", {"base": a})["code"] == -32602
    far = cad.execute(rect_cmd(500, 500, 600, 600))[0]
    assert cad.error("surface.intersect", {"ids": [a, far]})["code"] == 3


def test_execute_takes_the_new_commands_too(cad):
    r = cad.execute(rect_cmd(0, 0, 10, 10))[0]
    out = cad.result("execute", {"command": {"op": "transform_objects", "ids": [r], "transform": {"type": "move", "delta": [5, 5]}, "copy": True}})
    assert len(out["created"]) == 1
    out = cad.result("execute", {"command": {"op": "array_objects", "ids": [r], "array": {"shape": "linear", "count": 2, "offset": [100, 0]}}})
    assert len(out["created"]) == 2


def test_describe_lists_the_editing_methods(cad):
    desc = cad.result("describe")
    names = {m["name"] for m in desc["methods"]}
    assert {"edit.fillet", "edit.chamfer", "edit.trim", "edit.extend", "edit.split", "edit.offset", "edit.array",
            "edit.rotate", "edit.scale", "edit.mirror", "surface.add", "surface.subtract", "surface.clip",
            "surface.intersect", "surface.compose"} <= names
    assert desc["editing"]["examples"]
