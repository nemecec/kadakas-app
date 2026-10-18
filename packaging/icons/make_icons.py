#!/usr/bin/env python3
"""Draws the Kadakas icon and writes icon.svg, the PNG sizes, icon.ico and icon.icns.

The icon is our own: a juniper ("kadakas") as a landscape plan draws it, a canopy with a soft,
scalloped outline around a trunk dot, on a green tile. Four short ticks outside the canopy are
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
CANOPY = (0x7F, 0xB5, 0x8F)
INNER = (0x5D, 0x91, 0x70)
TICK = (0xFF, 0xFF, 0xFF)
ACCENT = (0xF2, 0xC1, 0x4E)

# Shapes in drawing order: (kind, parameters, color). A "blob" is a scalloped outline:
# (cx, cy, radius, lobes, lobe height, seed). The seed fixes the uneven lobe heights.
SHAPES = [
    ("rrect", (0, 0, 1024, 1024, 224), TILE),
    ("blob", (512, 512, 310, 26, 23, 7), CANOPY),
    ("blob", (512, 512, 200, 18, 17, 11), INNER),
    ("disc", (512, 512, 56), ACCENT),
    ("line", (512, 70, 512, 150, 30), TICK),
    ("line", (512, 874, 512, 954, 30), TICK),
    ("line", (70, 512, 150, 512, 30), TICK),
    ("line", (874, 512, 954, 512, 30), TICK),
]


def lobe_heights(lobes, height, seed):
    """Uneven heights, so the canopy looks grown and not like a gear. Fixed by the seed."""
    state = seed
    out = []
    for _ in range(lobes):
        state = (state * 1103515245 + 12345) % 2**31
        out.append(height * (0.6 + 0.8 * state / 2**31))
    return out


def blob_radius(p, angle):
    """The radius of a scalloped outline at an angle: one arch per lobe on top of the circle."""
    _, _, r, lobes, height, seed = p
    heights = lobe_heights(lobes, height, seed)
    t = (angle % (2 * math.pi)) / (2 * math.pi) * lobes
    i = int(t) % lobes
    return r + heights[i] * math.sin(math.pi * (t - int(t)))


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
        elif kind == "blob":
            cx, cy = p[0], p[1]
            pts = []
            for k in range(720):
                a = 2 * math.pi * k / 720
                rr = blob_radius(p, a)
                pts.append(f"{cx + rr * math.cos(a):.1f},{cy + rr * math.sin(a):.1f}")
            out.append(f'  <polygon points="{" ".join(pts)}" fill="{col}"/>')
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
    if kind == "blob":
        cx, cy = p[0], p[1]
        d = math.hypot(x - cx, y - cy)
        if d <= p[2]:
            return True
        return d <= blob_radius(p, math.atan2(y - cy, x - cx))
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
