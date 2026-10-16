"""Scenario: a designer gives the model materials, sets the sun and checks the 3D picture and the hidden-line drawing."""

import base64
import json
import statistics
import struct
import zlib

import pytest

from helpers import plant_cmd, read_doc, uid
from test_render_png import BACKGROUND, decode_png, render


def cube(x0, y0, side, thickness, fill=None):
    cmd = {
        "op": "add_object",
        "shape": {
            "kind": "hardscape",
            "boundary": [[x0, y0], [x0 + side, y0], [x0 + side, y0 + side], [x0, y0 + side]],
            "material": "Setts",
            "thickness": thickness,
            "elevation": thickness,
        },
    }
    if fill:
        cmd["attrs"] = {"fill": fill}
    return cmd


def lum(px):
    return 0.3 * px[0] + 0.59 * px[1] + 0.11 * px[2]


def png_bytes(width, height, rgb_rows):
    raw = b"".join(b"\x00" + bytes(row) for row in rgb_rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def test_describe_lists_the_presentation_methods_with_examples(cad):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    for m in [
        "material.list", "material.get", "material.create", "material.update", "material.duplicate",
        "material.delete", "material.add_defaults", "material.import_texture", "material.assign", "material.slots",
        "render_style.list", "render_style.get", "render_style.create", "render_style.update",
        "render_style.duplicate", "render_style.delete", "render_style.set_active", "sun.set", "sun.get",
        "render.hidden_line",
    ]:
        assert m in names, m
    examples = d["presentation"]["examples"]
    assert len(examples) >= 8
    assert all(e["method"] in names for e in examples)
    assert "hidden_line" in next(m["doc"] for m in d["methods"] if m["name"] == "render.png")


def test_materials_are_resources_with_a_default_library(cad):
    added = cad.result("material.add_defaults")["added"]
    assert {a["name"] for a in added} == {"Grass", "Gravel", "Concrete pavers", "Bark mulch", "Water", "Brick", "Timber"}
    assert cad.result("material.add_defaults")["added"] == []
    water = cad.result("material.get", {"id": "water"})
    assert water["opacity"] < 0.9 and water["roughness"] < 0.2
    assert water["texture"]["source"] == "procedural"
    # Resources appear in the document and survive undo and redo.
    assert any(r["name"] == "Brick" for r in cad.doc()["resources"])
    teak = cad.result("material.create", {
        "name": "Teak", "base_color": [150, 100, 60], "texture": {"procedural": "timber"},
        "mapping": {"kind": "planar", "scale": [1200, 300], "rotation": 10}, "roughness": 0.6, "metallic": 0.1,
    })["id"]
    assert cad.result("material.get", {"id": "Teak"})["mapping"]["scale"] == [1200.0, 300.0]
    cad.result("material.update", {"id": teak, "opacity": 0.8, "name": "Teak deck"})
    assert cad.result("material.get", {"id": teak})["opacity"] == pytest.approx(0.8)
    assert cad.result("undo")["done"]
    assert cad.result("material.get", {"id": teak})["name"] == "Teak"
    e = cad.error("material.create", {"name": "Bad", "roughness": 4})
    assert "roughness" in e["message"]
    assert "unknown pattern" in cad.error("material.create", {"name": "Bad", "texture": "marble"})["message"]


def test_images_are_stored_in_the_document_and_survive_saving(make_serve, tmp_path):
    cad = make_serve()
    rows = [[200, 40, 40] * 2 + [40, 40, 200] * 2 for _ in range(4)]
    b64 = base64.b64encode(png_bytes(4, 4, rows)).decode()
    mat = cad.result("material.create", {"name": "Photo", "texture": {"png_base64": b64}})["id"]
    tex = cad.result("material.get", {"id": mat})["texture"]
    assert (tex["source"], tex["width"], tex["height"]) == ("image", 4, 4)
    path = tmp_path / "doc.kad"
    cad.result("file.save", {"path": str(path)})
    other = make_serve()
    other.result("file.open", {"path": str(path)})
    assert other.result("material.get", {"id": "Photo"})["texture"] == tex
    assert "png_base64" in json.dumps(read_doc(path))
    # A file that is not an image is refused.
    bad = tmp_path / "x.png"
    bad.write_bytes(b"not an image")
    assert "PNG or JPEG" in cad.error("material.import_texture", {"id": mat, "path": str(bad)})["message"]
    # A PNG file on disk goes in with a size limit.
    good = tmp_path / "t.png"
    good.write_bytes(png_bytes(4, 4, rows))
    r = cad.result("material.import_texture", {"id": mat, "path": str(good), "max_side": 2})
    assert (r["width"], r["height"]) == (2, 2)


def test_textures_show_in_the_render_and_resolution_follows_the_order(cad):
    ground = cad.execute(cube(-4000, -4000, 8000, 10, fill=[200, 200, 200, 255]))[0]
    cad.result("material.add_defaults")
    flat = render(cad, view="top", width=160, height=160)[1]
    row = [flat[80][x] for x in range(40, 120)]
    assert max(lum(p) for p in row) - min(lum(p) for p in row) < 2
    # By class: the bark texture adds detail.
    cad.result("material.assign", {"material": "Concrete pavers", "class": uid(0)})
    textured = render(cad, view="top", width=160, height=160)[1]
    row = [textured[80][x] for x in range(40, 120)]
    assert statistics.pstdev(lum(p) for p in row) > 8
    # The object wins over the class: a plain color material has no detail.
    plain = cad.result("material.create", {"name": "Plaster", "base_color": [230, 230, 220]})["id"]
    cad.result("material.assign", {"material": plain, "object": ground})
    smooth = render(cad, view="top", width=160, height=160)[1]
    row = [smooth[80][x] for x in range(40, 120)]
    assert statistics.pstdev(lum(p) for p in row) < 2
    assert row[0][0] > 150
    slots = cad.result("material.slots")["slots"]
    assert [s["target"]["kind"] for s in slots] == ["class", "object"]
    # Clearing the object slot brings the class material back.
    cad.result("material.assign", {"material": None, "object": ground})
    again = render(cad, view="top", width=160, height=160)[1]
    assert again == textured
    assert cad.result("render.png", {"view": "top", "width": 160, "height": 160})["mode"] == "shaded"


def test_plant_parts_and_styles_take_materials(cad):
    style = cad.result("plant_style.create", {"name": "Oak", "spread": 4000})["id"]
    tree = cad.execute(plant_cmd(0, 0, spread=4000, height=8000))[0]
    cad.result("plants.set_style", {"ids": [tree], "style": style})
    cad.result("material.add_defaults")
    cad.result("material.assign", {"material": "Bark mulch", "style": "Oak", "slot": "trunk"})
    cad.result("material.assign", {"material": "Grass", "object": tree, "slot": "canopy"})
    slots = cad.result("material.slots")["slots"]
    assert {s["target"]["kind"] for s in slots} == {"style", "object"}
    img = cad.result("render.png", {"view": "front", "width": 200, "height": 200})
    w, h, rows = decode_png(img["png_base64"])
    greens = sum(1 for r in rows for p in r if p[1] > p[0] + 10 and p[1] > p[2] + 10)
    assert greens > 300, "the canopy has the grass material"
    assert "slot" in cad.error("material.assign", {"material": "Grass", "style": "Oak"})["message"]


def test_sun_position_follows_place_date_and_time(cad):
    assert cad.result("sun.get")["sun"] is None
    r = cad.result("sun.set", {"latitude": 60.17, "longitude": 24.94, "date": "2026-06-21", "time": "13:22", "tz_offset": 3})
    assert r["position"]["elevation"] == pytest.approx(53.3, abs=0.4)
    assert r["position"]["azimuth"] == pytest.approx(180.0, abs=3.0)
    assert r["day"]["solar_noon"] == "13:22"
    winter = cad.result("sun.set", {"date": "2026-12-21", "time": "12:00", "tz_offset": 2})
    assert winter["position"]["elevation"] == pytest.approx(6.4, abs=0.5)
    path = cad.result("sun.get", {"times": ["06:00", "12:00", "18:00"]})["path"]
    assert path[0]["azimuth"] < 180 < path[2]["azimuth"]
    assert cad.result("sun.set", {"off": True})["sun"] is None
    assert cad.result("undo")["done"]
    assert cad.result("sun.get")["sun"]["date"] == "2026-12-21"
    assert "latitude" in cad.error("sun.set", {"latitude": 123})["message"]
    # The sun can take its place from the georeference.
    cad.execute({"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000, 6672000]}})
    g = cad.result("sun.set", {"from_georef": True})
    assert 60.0 < g["sun"]["latitude"] < 60.4 and g["sun"]["from_georef"] is True


def test_realistic_style_casts_shadows_that_move_with_the_time(cad):
    cad.execute(cube(-6000, -6000, 12000, 10, fill=[200, 200, 200, 255]))
    cad.execute(cube(0, 0, 1500, 3000, fill=[170, 170, 170, 255]))
    cad.result("sun.set", {"latitude": 52.0, "longitude": 5.0, "tz_offset": 0, "date": "2026-03-20", "time": "09:00"})
    morning = render(cad, view="top", width=240, height=240, render_style="Realistic")[1]
    cad.result("sun.set", {"time": "15:00"})
    afternoon = render(cad, view="top", width=240, height=240, render_style="Realistic")[1]
    cad.result("render_style.create", {"name": "NoShadow", "sun": True, "shadows": False})
    unshaded = render(cad, view="top", width=240, height=240, render_style="NoShadow")[1]

    def darker(img):
        return [(x, y) for y in range(240) for x in range(240) if lum(img[y][x]) < lum(unshaded[y][x]) - 15]

    shadow_am, shadow_pm = darker(morning), darker(afternoon)
    assert len(shadow_am) > 100 and len(shadow_pm) > 100
    # The morning sun is in the east, so the shadow lies west (left); in the afternoon it lies east (right).
    mean_x = lambda pts: sum(x for x, _ in pts) / len(pts)
    assert mean_x(shadow_am) < 120 < mean_x(shadow_pm)
    # The render is the same on every run.
    assert render(cad, view="top", width=240, height=240, render_style="Realistic")[1] == afternoon
    # The built-in style names list.
    names = [s["name"] for s in cad.result("render_style.list")["builtin"]]
    assert {"Wireframe", "Shaded", "Realistic", "Hidden Line", "Dashed Hidden Line"} <= set(names)


def test_water_is_see_through(cad):
    cad.execute(cube(-4000, -4000, 8000, 10, fill=[220, 40, 40, 255]))
    pond = cad.execute(cube(-3000, -3000, 3000, 60, fill=[60, 120, 200, 255]))[0]
    cad.result("material.add_defaults")
    cad.result("material.assign", {"material": "Water", "object": pond})
    rows = render(cad, view="top", width=200, height=200)[1]
    dry, wet = rows[100][180], rows[100][70]
    assert dry[0] > 150 and dry[2] < 100
    assert wet[0] < dry[0] and wet[2] > dry[2] + 20, "the red ground shows through the blue water"
    assert wet[0] > 30, "some of the ground color is left"


def test_hidden_line_drawing_has_the_visible_edges_only(cad):
    cad.execute(cube(0, 0, 1000, 1000))
    r = cad.result("render.hidden_line", {"view": "right_isometric", "width": 400, "height": 400})
    assert r["visible_edges"] == 9
    assert r["svg"].count("<line") == 9
    d = cad.result("render.hidden_line", {"view": "right_isometric", "dashed": True, "segments": True, "mesh_edges": False})
    hidden = [s for s in d["segments"] if s["hidden"]]
    assert len(hidden) == 3 and len([s for s in d["segments"] if not s["hidden"]]) == 9
    assert "stroke-dasharray" in d["svg"]
    # Other directions give nine edges too, and the output does not change between runs.
    for view in ("left_isometric", "left_rear_isometric", "right_rear_isometric"):
        assert cad.result("render.hidden_line", {"view": view})["visible_edges"] == 9
    assert cad.result("render.hidden_line", {"view": "right_isometric"})["svg"] == cad.result(
        "render.hidden_line", {"view": "right_isometric"}
    )["svg"]
    # As a picture: dark lines on white. The covered edges are dashed in the other mode.
    for mode in ("hidden_line", "dashed_hidden_line"):
        img = cad.result("render.png", {"view": "right_isometric", "width": 200, "height": 200, "mode": mode})
        assert img["mode"] == mode
        _, _, rows = decode_png(img["png_base64"])
        white = sum(1 for row in rows for p in row if p[:3] == (255, 255, 255) or p[:3] == BACKGROUND[:3])
        assert white > 200 * 200 * 0.8
    assert "hidden_line" in cad.error("render.png", {"mode": "sketch"})["message"]


def test_render_styles_are_resources_and_the_active_one_applies(cad):
    cad.execute(cube(0, 0, 1000, 1000))
    plan = cad.result("render_style.create", {"name": "Plan lines", "mode": "hidden_line", "background": [255, 255, 255], "line_weight": 0.35})["id"]
    assert cad.result("render_style.get", {"id": "Plan lines"})["mode"] == "hidden_line"
    cad.result("render_style.set_active", {"id": plan})
    assert cad.result("render_style.list")["active"] == plan
    img = cad.result("render.png", {"view": "right_isometric", "width": 120, "height": 120})
    assert img["mode"] == "hidden_line"
    assert img["render_style"]["line_weight"] == 0.35
    # Deleting the active style clears it.
    cad.result("render_style.delete", {"id": plan})
    assert cad.result("render_style.list")["active"] is None
    assert "render_style" not in cad.result("render.png", {"view": "right_isometric", "width": 64, "height": 64})
    assert cad.result("undo")["done"]
    assert cad.result("render_style.list")["active"] == plan
    copy = cad.result("render_style.duplicate", {"id": plan})["id"]
    assert cad.result("render_style.get", {"id": copy})["name"] == "Plan lines copy"
    assert "unknown render style field" in cad.error("render_style.create", {"name": "x", "fog": 1})["message"]


def test_sheet_viewports_use_hidden_line_as_vector_lines(cad):
    cad.execute(cube(0, 0, 3000, 3000))
    cad.result("sheet.create", {"name": "Views", "paper": "a3"})
    vp = cad.result("viewport.create", {"sheet": "Views", "view": "right_isometric", "mode": "hidden_line", "scale": 50})
    svg = cad.result("sheet.render", {"sheet": "Views", "format": "svg"})["svg"]
    assert "<image" not in svg
    assert svg.count("<path") >= 9
    cad.result("viewport.update", {"id": vp["id"], "mode": "dashed_hidden_line"})
    dashed = cad.result("sheet.render", {"sheet": "Views", "format": "svg"})["svg"]
    assert "stroke-dasharray" in dashed
    # A render style on the viewport replaces its mode.
    style = cad.result("render_style.create", {"name": "Shaded sheet", "mode": "shaded"})["id"]
    cad.result("viewport.update", {"id": vp["id"], "render_style": style})
    assert "<image" in cad.result("sheet.render", {"sheet": "Views", "format": "svg"})["svg"]
    cad.result("viewport.update", {"id": vp["id"], "render_style": None})
    assert "stroke-dasharray" in cad.result("sheet.render", {"sheet": "Views", "format": "svg"})["svg"]
    assert "hidden_line" in cad.error("viewport.update", {"id": vp["id"], "mode": "sketchy"})["message"]
