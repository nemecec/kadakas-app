"""Scenario: a 3D viewport on a sheet draws the same scene as `render.png`: the terrain of the site
model, and the objects that stand on it."""

import base64
import struct
import zlib

import pytest


def decode_png(b64):
    """Decodes an 8-bit RGB or RGBA PNG. Returns (width, height, rows of RGB tuples)."""
    data = base64.b64decode(b64)
    pos, idat, width, height, channels = 8, b"", 0, 0, 4
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind, body = data[pos + 4 : pos + 8], data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color = struct.unpack(">IIBB", body[:10])
            assert depth == 8 and color in (2, 6), (depth, color)
            channels = 3 if color == 2 else 4
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride = width * channels
    rows, prev = [], bytearray(stride)
    for y in range(height):
        base = y * (stride + 1)
        ft, line = raw[base], bytearray(raw[base + 1 : base + 1 + stride])
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b = prev[i]
            c = prev[i - channels] if i >= channels else 0
            if ft == 1:
                line[i] = (line[i] + a) & 255
            elif ft == 2:
                line[i] = (line[i] + b) & 255
            elif ft == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 255
            elif ft == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else (b if pb <= pc else c))) & 255
        rows.append([tuple(line[x * channels : x * channels + 3]) for x in range(width)])
        prev = line
    return width, height, rows


def terrain_site(cad):
    ids = []
    for x in range(5):
        for y in range(5):
            ids += cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 10000, y * 10000, 30000 + x * 500 + y * 300]}})
    return cad.result("site.create", {"source_ids": ids})["id"]


def painted(rows):
    """The number of pixels that are not paper white."""
    return sum(1 for row in rows for px in row if px != (255, 255, 255))


@pytest.mark.parametrize("mode", ["shaded", "hidden_line", "dashed_hidden_line"])
def test_a_3d_viewport_draws_the_terrain(cad, mode):
    terrain_site(cad)
    cad.result("sheet.create", {"name": "S", "paper": "a3"})
    cad.result("viewport.create", {"sheet": "S", "view": "right_isometric", "scale": "fit", "mode": mode})
    sheet = cad.result("sheet.render", {"sheet": "S", "format": "png", "dpi": 60})
    w, h, rows = decode_png(sheet["png_base64"])
    # Shaded fills the terrain. The line modes draw thin lines.
    share = 0.02 if mode == "shaded" else 0.001
    assert painted(rows) > share * w * h, f"the {mode} viewport is blank"
    # The 3D view of the model shows terrain too.
    view = cad.result("render.png", {"view": "right_isometric", "width": 300, "height": 200})
    _, _, vrows = decode_png(view["png_base64"])
    assert painted(vrows) > 0


def test_a_3d_viewport_draws_the_objects_on_the_terrain(cad):
    terrain_site(cad)
    cad.result("sheet.create", {"name": "S", "paper": "a3"})
    cad.result("viewport.create", {"sheet": "S", "view": "right_isometric", "scale": "fit", "mode": "shaded"})
    w, h, bare = decode_png(cad.result("sheet.render", {"sheet": "S", "format": "png", "dpi": 60})["png_base64"])
    # A building on the terrain and a wall: the picture has more in it than the terrain alone.
    cad.result("massing.add", {"footprint": [[10000, 10000], [20000, 10000], [20000, 20000], [10000, 20000]], "height": 12000})
    cad.result("wall.add", {"path": [[5000, 30000], [35000, 30000]], "top": {"mode": "terrain", "offset": 3000}})
    _, _, rows = decode_png(cad.result("sheet.render", {"sheet": "S", "format": "png", "dpi": 60})["png_base64"])
    assert bare and rows != bare
    assert sum(1 for r1, r2 in zip(rows, bare) for p, q in zip(r1, r2) if p != q) > 0.005 * w * h
