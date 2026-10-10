#!/usr/bin/env python3
"""Draws the Cadektor icon and writes icon.svg, the PNG sizes, icon.ico and icon.icns.

The icon is our own: a plan symbol of a tree (a ring with a trunk dot) on a green tile, with
the crosshair of a CAD cursor. One list of shapes drives both the SVG and the pixel renderer, so
they always match. Only the Python standard library is used.

Run:  python3 packaging/icons/make_icons.py
"""
import math
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
SIZE = 1024  # the design grid

TILE = (0x2F, 0x6B, 0x4F)
CANOPY = (0xFF, 0xFF, 0xFF)
ACCENT = (0xF2, 0xC1, 0x4E)

# Shapes in drawing order: (kind, parameters, color).
SHAPES = [
    ("rrect", (0, 0, 1024, 1024, 224), TILE),
    ("ring", (512, 512, 300, 56), CANOPY),
    ("line", (120, 512, 904, 512, 26), CANOPY),
    ("line", (512, 120, 512, 904, 26), CANOPY),
    ("disc", (512, 512, 92), ACCENT),
]


def hexcolor(c):
    return "#%02X%02X%02X" % c


def svg():
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}" width="{SIZE}" height="{SIZE}">']
    for kind, p, c in SHAPES:
        col = hexcolor(c)
        if kind == "rrect":
            x, y, w, h, r = p
            out.append(f'  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{col}"/>')
        elif kind == "ring":
            cx, cy, r, sw = p
            out.append(f'  <circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{col}" stroke-width="{sw}"/>')
        elif kind == "line":
            x1, y1, x2, y2, sw = p
            out.append(f'  <line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{col}" stroke-width="{sw}" stroke-linecap="round"/>')
        elif kind == "disc":
            cx, cy, r = p
            out.append(f'  <circle cx="{cx}" cy="{cy}" r="{r}" fill="{col}"/>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


def inside(kind, p, x, y):
    if kind == "rrect":
        x0, y0, w, h, r = p
        qx = abs(x - (x0 + w / 2)) - (w / 2 - r)
        qy = abs(y - (y0 + h / 2)) - (h / 2 - r)
        return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) <= r
    if kind == "ring":
        cx, cy, r, sw = p
        return abs(math.hypot(x - cx, y - cy) - r) <= sw / 2
    if kind == "disc":
        cx, cy, r = p
        return math.hypot(x - cx, y - cy) <= r
    if kind == "line":
        x1, y1, x2, y2, sw = p
        dx, dy = x2 - x1, y2 - y1
        t = max(0.0, min(1.0, ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy)))
        return math.hypot(x - (x1 + t * dx), y - (y1 + t * dy)) <= sw / 2
    raise ValueError(kind)


def render(n, ss=3):
    """RGBA bytes of an n x n icon, with ss x ss samples per pixel."""
    scale = SIZE / n
    rows = []
    for py in range(n):
        row = bytearray()
        for px in range(n):
            r = g = b = a = 0
            for sy in range(ss):
                for sx in range(ss):
                    x = (px + (sx + 0.5) / ss) * scale
                    y = (py + (sy + 0.5) / ss) * scale
                    hit = None
                    for kind, p, c in SHAPES:
                        if inside(kind, p, x, y):
                            hit = c
                    if hit:
                        r += hit[0]
                        g += hit[1]
                        b += hit[2]
                        a += 1
            if a:
                row += bytes((r // a, g // a, b // a, 255 * a // (ss * ss)))
            else:
                row += bytes(4)
        rows.append(bytes(row))
    return rows


def png(n):
    rows = render(n)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + r for r in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico(images):
    """images: {size: png bytes}. PNG-compressed entries, as modern Windows reads them."""
    sizes = sorted(images)
    head = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries, data = b"", b""
    for s in sizes:
        d = images[s]
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(d), offset + len(data))
        data += d
    return head + entries + data


def icns(images):
    """images: {size: png bytes} for 128, 256 and 512 (types ic07, ic08, ic09)."""
    kinds = {128: b"ic07", 256: b"ic08", 512: b"ic09"}
    body = b"".join(kinds[s] + struct.pack(">I", 8 + len(images[s])) + images[s] for s in sorted(kinds))
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def main():
    with open(os.path.join(HERE, "icon.svg"), "w") as f:
        f.write(svg())
    sizes = [16, 32, 48, 64, 128, 256, 512]
    images = {s: png(s) for s in sizes}
    for s in (32, 64, 128, 256, 512):
        with open(os.path.join(HERE, f"{s}x{s}.png"), "wb") as f:
            f.write(images[s])
    with open(os.path.join(HERE, "icon.ico"), "wb") as f:
        f.write(ico({s: images[s] for s in (16, 32, 48, 64, 128, 256)}))
    with open(os.path.join(HERE, "icon.icns"), "wb") as f:
        f.write(icns(images))
    print("wrote icons in", HERE)


if __name__ == "__main__":
    main()
