"""Scenario: an agent looks at the 3D model through `render.png`, and switches views with `view.set`."""

import base64
import json
import os
import struct
import subprocess
import zlib

import pytest

from helpers import BIN, line_cmd, plant_cmd
from test_connect import FakeApp

BACKGROUND = (246, 247, 249, 255)
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def decode_png(b64: str):
    """Decodes an 8-bit RGBA PNG with the standard library. Returns (width, height, rows of pixel tuples)."""
    data = base64.b64decode(b64)
    assert data[:8] == PNG_SIGNATURE, "not a PNG"
    pos, chunks, idat = 8, [], b""
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        chunks.append(kind)
        if kind == b"IHDR":
            width, height, depth, color = struct.unpack(">IIBB", body[:10])
            assert (depth, color) == (8, 6), "expected 8-bit RGBA"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    assert chunks[0] == b"IHDR" and chunks[-1] == b"IEND"
    raw = zlib.decompress(idat)
    stride = width * 4
    rows, prev = [], bytearray(stride)
    for y in range(height):
        base = y * (stride + 1)
        ft, line = raw[base], bytearray(raw[base + 1 : base + 1 + stride])
        for i in range(stride):
            a = line[i - 4] if i >= 4 else 0
            b = prev[i]
            c = prev[i - 4] if i >= 4 else 0
            if ft == 1:
                line[i] = (line[i] + a) & 255
            elif ft == 2:
                line[i] = (line[i] + b) & 255
            elif ft == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 255
            elif ft == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 255
            else:
                assert ft == 0, f"bad filter {ft}"
        rows.append([tuple(line[x * 4 : x * 4 + 4]) for x in range(width)])
        prev = line
    return width, height, rows


def render(cad, **params):
    r = cad.result("render.png", params)
    w, h, rows = decode_png(r["png_base64"])
    assert (w, h) == (r["width"], r["height"])
    return r, rows


def box_of(rows, predicate):
    """Pixel bounding box (x0, y0, x1, y1) of the pixels that satisfy `predicate`, or None."""
    xs, ys = [], []
    for y, row in enumerate(rows):
        for x, px in enumerate(row):
            if predicate(px):
                xs.append(x)
                ys.append(y)
    return (min(xs), min(ys), max(xs), max(ys)) if xs else None


def hardscape(x0, y0, x1, y1, fill=None):
    cmd = {
        "op": "add_object",
        "shape": {"kind": "hardscape", "boundary": [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], "material": "Setts"},
    }
    if fill:
        cmd["attrs"] = {"fill": fill}
    return cmd


def test_render_png_is_a_valid_png_of_the_asked_size(cad):
    cad.execute(hardscape(0, 0, 4000, 3000))
    r = cad.result("render.png", {"width": 320, "height": 200})
    raw = base64.b64decode(r["png_base64"])
    assert raw[:8] == PNG_SIGNATURE
    assert struct.unpack(">II", raw[16:24]) == (320, 200)
    assert (r["width"], r["height"]) == (320, 200)


def test_render_png_defaults_to_800_by_600_and_limits_the_size(cad):
    assert struct.unpack(">II", base64.b64decode(cad.result("render.png")["png_base64"])[16:24]) == (800, 600)
    r = cad.result("render.png", {"width": 1, "height": 99999})
    assert (r["width"], r["height"]) == (16, 2048), "2048 px per side at most"
    r = cad.result("render.png", {"width": 4000, "height": 4000})
    assert r["width"] * r["height"] <= 4_000_000, "4 million pixels at most"
    assert r["width"] == r["height"] and r["width"] >= 1900, "scaled down, keeping the shape"


def test_plan_and_top_look_the_same_in_a_render(cad):
    cad.execute(hardscape(0, 0, 4000, 3000))
    assert render(cad, view="plan", width=64, height=48)[1] == render(cad, view="top", width=64, height=48)[1]


def test_top_view_of_a_hardscape_covers_the_expected_pixels(cad):
    cad.execute(hardscape(0, 0, 4000, 3000, fill=[220, 0, 0, 255]))
    r, rows = render(cad, view="top", width=400, height=300)
    assert r["view"] == "top"
    red = box_of(rows, lambda p: p[0] > 150 and p[1] < 60 and p[2] < 60)
    assert red is not None
    # The slab is 4000 x 3000 mm and fills the view with a margin of about 8 percent.
    x0, y0, x1, y1 = red
    assert 12 <= x0 <= 22 and 10 <= y0 <= 18, red
    assert 377 <= x1 <= 388 and 281 <= y1 <= 290, red
    # Corners are background, the middle is the slab.
    assert rows[2][2] == BACKGROUND and rows[297][397] == BACKGROUND
    assert rows[150][200][0] > 150


def test_top_view_has_north_up(cad):
    cad.execute(line_cmd(0, 0, 4000, 0))
    cad.execute(hardscape(0, 2000, 1000, 3000, fill=[0, 0, 220, 255]))
    _, rows = render(cad, view="top", width=400, height=300)
    blue = box_of(rows, lambda p: p[2] > 150 and p[0] < 60 and p[1] < 60)
    assert blue is not None
    x0, y0, x1, y1 = blue
    assert y1 < 150, "+Y is the upper half"
    assert x1 < 200, "the slab is at the left, since X grows to the right"


def test_a_hidden_layer_gives_an_empty_image(cad):
    cad.execute(hardscape(0, 0, 4000, 3000))
    _, rows = render(cad, width=64, height=48)
    assert box_of(rows, lambda p: p != BACKGROUND) is not None
    cad.execute({"op": "modify_layer", "id": 1, "visible": False})
    _, rows = render(cad, width=64, height=48)
    assert box_of(rows, lambda p: p != BACKGROUND) is None, "every pixel is background"
    cad.execute({"op": "modify_layer", "id": 1, "visible": True})
    _, rows = render(cad, width=64, height=48)
    assert box_of(rows, lambda p: p != BACKGROUND) is not None


def test_a_hidden_class_is_not_drawn(cad):
    hidden = cad.execute({"op": "add_class", "name": "Hidden"})[0]
    cad.execute(hardscape(0, 0, 4000, 3000) | {"class": hidden})
    cad.execute({"op": "modify_class", "id": hidden, "visible": False})
    _, rows = render(cad, width=64, height=48)
    assert box_of(rows, lambda p: p != BACKGROUND) is None


def test_an_empty_drawing_is_all_background(cad):
    _, rows = render(cad, width=32, height=24)
    assert all(px == BACKGROUND for row in rows for px in row)


def test_view_list_names_the_standard_views(cad):
    v = cad.result("view.list")
    ids = [x["id"] for x in v["views"]]
    assert ids[0] == "plan" and "top" in ids
    assert {"front", "back", "left", "right", "left_isometric", "right_isometric", "left_rear_isometric", "right_rear_isometric"} <= set(ids)
    assert set(v["projections"]) == {"ortho", "perspective"}
    assert set(v["modes"]) == {"shaded", "wireframe", "hidden_line", "dashed_hidden_line"}
    assert v["current"] == {"view": "plan", "projection": "ortho", "mode": "shaded"}


@pytest.mark.parametrize("view", ["plan", "top", "bottom", "front", "back", "left", "right", "left_isometric", "right_isometric", "left_rear_isometric", "right_rear_isometric"])
def test_every_standard_view_renders_a_plant(cad, view):
    cad.execute(plant_cmd(0, 0, spread=4000, height=8000))
    r, rows = render(cad, view=view, width=120, height=120)
    assert r["view"] == view
    assert box_of(rows, lambda p: p != BACKGROUND) is not None


def test_front_view_shows_height_and_top_view_does_not(cad):
    cad.execute(plant_cmd(0, 0, spread=2000, height=8000))
    _, front = render(cad, view="front", width=200, height=200)
    _, top = render(cad, view="top", width=200, height=200)
    fx0, fy0, fx1, fy1 = box_of(front, lambda p: p != BACKGROUND)
    tx0, ty0, tx1, ty1 = box_of(top, lambda p: p != BACKGROUND)
    assert (fy1 - fy0) > 2.5 * (fx1 - fx0), "a tall thin tree in the front view"
    assert abs((ty1 - ty0) - (tx1 - tx0)) <= 2, "a round canopy from above"


def test_wireframe_and_perspective_change_the_picture(cad):
    cad.execute(hardscape(0, 0, 4000, 3000, fill=[200, 20, 20, 255]))
    _, shaded = render(cad, view="right_isometric", width=100, height=100)
    r, wire = render(cad, view="right_isometric", width=100, height=100, mode="wireframe")
    assert r["mode"] == "wireframe"
    filled = lambda rows: sum(1 for row in rows for p in row if p != BACKGROUND)
    assert filled(wire) < filled(shaded) / 3, "wireframe has lines only"
    r, persp = render(cad, view="right_isometric", width=100, height=100, projection="perspective")
    assert r["projection"] == "perspective"
    assert persp != shaded


def test_view_set_changes_what_render_png_shows_by_default(cad):
    cad.execute(plant_cmd(0, 0, spread=2000, height=8000))
    assert cad.result("view.set", {"view": "Front"}) == {"view": "front", "projection": "ortho", "mode": "shaded"}
    r, rows = render(cad, width=100, height=100)
    assert r["view"] == "front"
    x0, y0, x1, y1 = box_of(rows, lambda p: p != BACKGROUND)
    assert (y1 - y0) > 2 * (x1 - x0)
    cad.result("view.set", {"projection": "perspective", "mode": "wireframe"})
    assert cad.result("view.list")["current"] == {"view": "front", "projection": "perspective", "mode": "wireframe"}


def test_bad_view_arguments_are_invalid_params(cad):
    for method in ["render.png", "view.set"]:
        for params in [{"view": "sideways"}, {"projection": "fisheye"}, {"mode": "xray"}, {"view": 5}]:
            assert cad.error(method, params)["code"] == -32602, (method, params)
    assert "right_isometric" in cad.error("view.set", {"view": "sideways"})["message"]
    assert cad.error("view.set", {})["code"] == -32602, "view.set needs something to set"
    assert cad.error("render.png", {"width": "wide"})["code"] == -32602


def test_render_png_follows_the_document(cad):
    cad.execute(hardscape(0, 0, 1000, 1000))
    _, before = render(cad, view="top", width=64, height=64)
    cad.execute(hardscape(0, 0, 9000, 9000, fill=[0, 120, 0, 255]))
    _, after = render(cad, view="top", width=64, height=64)
    assert before != after
    cad.result("undo")
    _, undone = render(cad, view="top", width=64, height=64)
    assert undone == before


def test_describe_lists_the_new_methods(cad):
    names = {m["name"] for m in cad.result("describe")["methods"]}
    assert {"render.png", "view.list", "view.set"} <= names


def test_mcp_lists_the_new_tools_and_returns_the_image_as_an_image(mcp):
    tools = {tool["name"]: tool for tool in mcp.request("tools/list")["result"]["tools"]}
    assert {"render.png", "view.list", "view.set"} <= set(tools["render_view"]["inputSchema"]["properties"]["method"]["enum"])
    mcp.method_json("document", "execute", {"command": hardscape(0, 0, 4000, 3000)})
    result = mcp.method("render_view", "render.png", {"view": "right_isometric", "width": 64, "height": 48})
    assert result["isError"] is False
    image, meta = result["content"]
    assert image["type"] == "image" and image["mimeType"] == "image/png"
    assert decode_png(image["data"])[:2] == (64, 48)
    assert json.loads(meta["text"]) == {"width": 64, "height": 48, "view": "right_isometric", "projection": "ortho", "mode": "shaded"}
    assert mcp.method_json("render_view", "view.set", {"view": "back"})["view"] == "back"
    assert mcp.method_json("render_view", "view.list")["current"]["view"] == "back"
    bad = mcp.method("render_view", "render.png", {"view": "nope"})
    assert bad["isError"] is True and "unknown view" in bad["content"][0]["text"]


def test_flat_mcp_lists_one_tool_per_method_and_returns_the_image(mcp_flat):
    names = {tool["name"] for tool in mcp_flat.request("tools/list")["result"]["tools"]}
    assert {"render_png", "view_list", "view_set"} <= names
    mcp_flat.tool_json("execute", {"command": hardscape(0, 0, 4000, 3000)})
    result = mcp_flat.tool("render_png", {"view": "top", "width": 32, "height": 24})
    assert result["content"][0]["type"] == "image"


def test_render_png_and_view_set_are_forwarded_to_the_app_with_connect():
    app = FakeApp()
    reqs = [
        {"jsonrpc": "2.0", "id": 1, "method": "render.png", "params": {"view": "top", "width": 64}},
        {"jsonrpc": "2.0", "id": 2, "method": "view.set", "params": {"view": "front"}},
    ]
    p = subprocess.run(
        [str(BIN), "serve", "--connect", f"127.0.0.1:{app.port}", "--token", "s3cret"],
        input="".join(json.dumps(r) + "\n" for r in reqs),
        capture_output=True,
        text=True,
        timeout=20,
        env=os.environ,
    )
    assert p.returncode == 0, p.stderr
    assert [json.loads(l)["result"] for l in p.stdout.splitlines()] == [{"echo": "render.png"}, {"echo": "view.set"}]
    assert [l["method"] for l in app.lines] == ["auth", "render.png", "view.set"]
    assert app.lines[1]["params"] == {"view": "top", "width": 64}
