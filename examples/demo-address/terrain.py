"""Terrain grid helpers: clean the grid, triangles for a surface model, cut and fill volumes.

The grid has the shape of `sources_extra.terrain_grid`: {origin: (x, y), step, nx, ny, z: [[...]]}.
`z[j][i]` is the height in metres at x = origin_x + i * step, y = origin_y + j * step. A node
without data is None. The origin is the south-west node."""

from __future__ import annotations

from geom import Pt, point_in_poly


def clean_grid(grid: dict) -> dict | None:
    """Fill nodes without data with the mean of the others. None when there is too little data."""
    rows = grid.get("z") or []
    values = [v for row in rows for v in row if v is not None]
    ny = len(rows)
    nx = len(rows[0]) if rows else 0
    if nx < 2 or ny < 2 or len(values) < 0.8 * nx * ny:
        return None
    mean = sum(values) / len(values)
    z = [[float(v) if v is not None else mean for v in row] for row in rows]
    return {"origin": tuple(grid["origin"]), "step": float(grid["step"]), "nx": nx, "ny": ny, "z": z}


def z_at(grid: dict, x: float, y: float) -> float:
    """Bilinear height. Points outside the grid take the nearest edge."""
    fx = (x - grid["origin"][0]) / grid["step"]
    fy = (y - grid["origin"][1]) / grid["step"]
    fx = min(max(fx, 0.0), grid["nx"] - 1.000001)
    fy = min(max(fy, 0.0), grid["ny"] - 1.000001)
    i, j = int(fx), int(fy)
    tx, ty = fx - i, fy - j
    z = grid["z"]
    return (z[j][i] * (1 - tx) * (1 - ty) + z[j][i + 1] * tx * (1 - ty)
            + z[j + 1][i] * (1 - tx) * ty + z[j + 1][i + 1] * tx * ty)


def triangles(grid: dict, z0: float = 0.0) -> list[list[tuple[float, float, float]]]:
    """Two triangles per cell, as (x, y, z) in metres with z relative to z0. Counterclockwise from above."""
    out = []
    ox, oy = grid["origin"]
    s = grid["step"]
    z = grid["z"]
    for j in range(grid["ny"] - 1):
        for i in range(grid["nx"] - 1):
            p00 = (ox + i * s, oy + j * s, z[j][i] - z0)
            p10 = (ox + (i + 1) * s, oy + j * s, z[j][i + 1] - z0)
            p01 = (ox + i * s, oy + (j + 1) * s, z[j + 1][i] - z0)
            p11 = (ox + (i + 1) * s, oy + (j + 1) * s, z[j + 1][i + 1] - z0)
            out.append([p00, p10, p11])
            out.append([p00, p11, p01])
    return out


def cut_fill(grid: dict, level: float, area: list[Pt] | None = None, sub: int = 8) -> dict:
    """Cut (ground above the level) and fill (ground below) volumes in m3 for a flat level.

    Each cell is sampled `sub` x `sub` times with the bilinear height. With `area`, only samples
    inside the polygon count. Returns {cut, fill, net, area}; net = cut - fill (positive means
    soil goes away)."""
    s = grid["step"]
    ox, oy = grid["origin"]
    cell_area = (s / sub) ** 2
    cut = fill = counted = 0.0
    for j in range(grid["ny"] - 1):
        for i in range(grid["nx"] - 1):
            for b in range(sub):
                for a in range(sub):
                    x = ox + (i + (a + 0.5) / sub) * s
                    y = oy + (j + (b + 0.5) / sub) * s
                    if area is not None and not point_in_poly((x, y), area):
                        continue
                    d = z_at(grid, x, y) - level
                    counted += cell_area
                    if d > 0:
                        cut += d * cell_area
                    else:
                        fill -= d * cell_area
    return {"cut": cut, "fill": fill, "net": cut - fill, "area": counted}


def balance_level(grid: dict, area: list[Pt] | None = None) -> float:
    """The flat level where cut equals fill (bisection). With a flat sampling this is the mean height."""
    values = [v for row in grid["z"] for v in row]
    lo, hi = min(values), max(values)
    for _ in range(40):
        mid = (lo + hi) / 2
        if cut_fill(grid, mid, area, sub=2)["net"] > 0:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2
