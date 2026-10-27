"""Pure planning of the yard and the extension. No Kadakas calls here, so it is easy to test.

All coordinates are metres in the map frame (EPSG:3301) or in any other frame, as long as the
inputs agree."""

from __future__ import annotations

import math
from dataclasses import dataclass, field

import geom
from geom import Pt

DEFAULT_PLOT_MARGIN = 8.0     # m, offset of the footprint when no plot boundary is known
EXT_DEPTH = 5.0               # m, out from the facade
EXT_LENGTH = 8.0              # m, along the facade
EDGE_MARGIN = 1.5             # m, free strip kept at the plot border
MIN_STOREY = 2.7              # m, lowest storey height we assume for the extension


@dataclass
class Layout:
    frame: dict
    plot: list[Pt]
    plot_is_assumed: bool
    sides: dict[str, float]
    extension: list[Pt] | None = None
    extension_side: str | None = None
    extension_note: str = ""
    terrace: list[Pt] | None = None
    path: list[Pt] | None = None
    parking: list[Pt] | None = None
    trees: list[Pt] = field(default_factory=list)
    shrubs: list[Pt] = field(default_factory=list)
    hedge: list[Pt] | None = None

    @property
    def reserved(self) -> list[list[Pt]]:
        return [r for r in (self.extension, self.terrace, self.path, self.parking) if r]


def assumed_plot(footprint: list[Pt], margin: float = DEFAULT_PLOT_MARGIN) -> list[Pt]:
    """No plot boundary known: the rectangle around the footprint, `margin` m out on each side."""
    frame = geom.min_rect(footprint)
    return geom.expand_rect(frame, margin)


def _side_rect(frame: dict, side: str, depth: float, length: float, touch: float | None = None) -> list[Pt]:
    """A rectangle on one side of the frame, `depth` out from the side and `length` along it, centred."""
    axis = side[0]
    sign = 1 if side[1] == "+" else -1
    if axis == "v":  # facade runs along u, out along v
        mid = (frame["lo_u"] + frame["hi_u"]) / 2
        a0, a1 = mid - length / 2, mid + length / 2
        edge = (frame["hi_v"] if sign > 0 else frame["lo_v"]) if touch is None else touch
        b0, b1 = (edge, edge + depth) if sign > 0 else (edge - depth, edge)
        return geom.rect_in_frame(frame, a0, a1, b0, b1)
    mid = (frame["lo_v"] + frame["hi_v"]) / 2
    b0, b1 = mid - length / 2, mid + length / 2
    edge = (frame["hi_u"] if sign > 0 else frame["lo_u"]) if touch is None else touch
    a0, a1 = (edge, edge + depth) if sign > 0 else (edge - depth, edge)
    return geom.rect_in_frame(frame, a0, a1, b0, b1)


def _facade_length(frame: dict, side: str) -> float:
    return frame["length"] if side[0] == "v" else frame["width"]


def plan_extension(frame: dict, footprint: list[Pt], plot: list[Pt], sides: dict[str, float]) -> tuple[list[Pt] | None, str | None, str]:
    """The extension rectangle on the side with the most free space.

    Returns (rectangle or None, side, note). The note says what was adjusted."""
    order = sorted(sides, key=lambda s: (-sides[s], s))
    for side in order:
        length = min(EXT_LENGTH, _facade_length(frame, side))
        depth = EXT_DEPTH
        note = ""
        free = sides[side]
        if free - EDGE_MARGIN < depth:
            depth = round(free - EDGE_MARGIN, 1)
            note = "Laiendus on tavalisest väiksema sügavusega, sest krundil pole ruumi."
        if depth < 2.5 or length < 2.5:
            continue
        # touch the building: the facade is the outermost footprint point inside the span
        rect = _side_rect(frame, side, depth, length)
        touch = _touch_edge(frame, footprint, side, rect)
        rect = _side_rect(frame, side, depth, length, touch=touch)
        if geom.inset_ok(rect, plot, margin=EDGE_MARGIN * 0.5):
            return rect, side, note
    return None, None, "Krundil ei leidu kohta 2,5 m sügavusele laiendusele."


def _touch_edge(frame: dict, footprint: list[Pt], side: str, rect: list[Pt]) -> float:
    """Where the extension meets the building: the footprint boundary point that sticks out furthest
    on that side, within the span of the rectangle."""
    axis, sign = side[0], 1 if side[1] == "+" else -1
    lo, hi = (frame["lo_u"], frame["hi_u"]) if axis == "v" else (frame["lo_v"], frame["hi_v"])
    outer = frame["hi_v"] if (axis == "v" and sign > 0) else frame["lo_v"] if axis == "v" else \
        frame["hi_u"] if sign > 0 else frame["lo_u"]
    # span of the rectangle along the facade
    spans = []
    for p in rect:
        spans.append(p[0] * frame["u"][0] + p[1] * frame["u"][1] if axis == "v"
                     else p[0] * frame["v"][0] + p[1] * frame["v"][1])
    s0, s1 = min(spans), max(spans)
    best = None
    n = len(footprint)
    for i in range(n):
        a, b = footprint[i], footprint[(i + 1) % n]
        for t in [k / 20 for k in range(21)]:
            x, y = a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t
            u = x * frame["u"][0] + y * frame["u"][1]
            v = x * frame["v"][0] + y * frame["v"][1]
            along, across = (u, v) if axis == "v" else (v, u)
            if s0 <= along <= s1:
                val = across * sign
                if best is None or val > best:
                    best = val
    if best is None:
        return outer
    return best * sign


def plan_yard(footprint: list[Pt], plot: list[Pt], plot_is_assumed: bool, height_hint: float = 6.0) -> Layout:
    frame = geom.min_rect(footprint)
    sides = geom.free_space(frame, plot)
    ext, ext_side, note = plan_extension(frame, footprint, plot, sides)
    lay = Layout(frame=frame, plot=plot, plot_is_assumed=plot_is_assumed, sides=sides,
                 extension=ext, extension_side=ext_side, extension_note=note)

    # terrace: the second best side (the best one is for the extension)
    order = sorted(sides, key=lambda s: (-sides[s], s))
    candidates = [s for s in order if s != ext_side] or order
    for side in candidates:
        free = sides[side]
        if free < 5.0:
            continue
        depth = 3.5
        length = min(5.0, _facade_length(frame, side))
        rect = _side_rect(frame, side, depth, length)
        touch = _touch_edge(frame, footprint, side, rect)
        rect = _side_rect(frame, side, depth, length, touch=touch + (0.0 if side[1] == "+" else 0.0))
        if geom.inset_ok(rect, plot, EDGE_MARGIN) and not any(geom.rects_overlap(rect, r) for r in [ext] if r):
            lay.terrace = rect
            lay.path, lay.parking = _path_and_parking(frame, side, rect, plot, lay.reserved)
            break

    lay.hedge = _hedge(plot)
    lay.trees = _pick_points(lay, footprint, 3, clearance=3.0, spread=4.5)
    lay.shrubs = _pick_points(lay, footprint, 5, clearance=1.8, spread=1.5)
    return lay


def _path_and_parking(frame: dict, side: str, terrace: list[Pt], plot: list[Pt], reserved: list[list[Pt]]):
    """A 1.2 m path from the terrace straight out to 1.5 m before the plot border, and a parking pad beside its end."""
    axis, sign = side[0], 1 if side[1] == "+" else -1
    cx = sum(p[0] for p in terrace) / 4
    cy = sum(p[1] for p in terrace) / 4
    out = frame["v"] if axis == "v" else frame["u"]
    out = (out[0] * sign, out[1] * sign)
    side_dir = (-out[1], out[0])
    # start at the far edge of the terrace
    half = 3.5 / 2
    sx, sy = cx + out[0] * half, cy + out[1] * half
    d = 0.0
    while geom.point_in_poly((sx + out[0] * (d + 0.5), sy + out[1] * (d + 0.5)), plot) and d < 60:
        d += 0.5
    length = d - EDGE_MARGIN
    if length < 2.0:
        return None, None
    w = 0.6
    path = [(sx - side_dir[0] * w, sy - side_dir[1] * w), (sx + side_dir[0] * w, sy + side_dir[1] * w),
            (sx + side_dir[0] * w + out[0] * length, sy + side_dir[1] * w + out[1] * length),
            (sx - side_dir[0] * w + out[0] * length, sy - side_dir[1] * w + out[1] * length)]
    parking = None
    if length >= 5.5:
        # a pad 2.5 m wide and 5 m long, beside the path end
        off = w + 0.3
        a = (sx + side_dir[0] * off + out[0] * (length - 5.0), sy + side_dir[1] * off + out[1] * (length - 5.0))
        pad = [a, (a[0] + side_dir[0] * 2.5, a[1] + side_dir[1] * 2.5),
               (a[0] + side_dir[0] * 2.5 + out[0] * 5.0, a[1] + side_dir[1] * 2.5 + out[1] * 5.0),
               (a[0] + out[0] * 5.0, a[1] + out[1] * 5.0)]
        if geom.inset_ok(pad, plot, EDGE_MARGIN * 0.5) and not any(geom.rects_overlap(pad, r) for r in reserved):
            parking = pad
        else:  # try the other side of the path
            a = (sx - side_dir[0] * (off + 2.5) + out[0] * (length - 5.0), sy - side_dir[1] * (off + 2.5) + out[1] * (length - 5.0))
            pad = [a, (a[0] + side_dir[0] * 2.5, a[1] + side_dir[1] * 2.5),
                   (a[0] + side_dir[0] * 2.5 + out[0] * 5.0, a[1] + side_dir[1] * 2.5 + out[1] * 5.0),
                   (a[0] + out[0] * 5.0, a[1] + out[1] * 5.0)]
            if geom.inset_ok(pad, plot, EDGE_MARGIN * 0.5) and not any(geom.rects_overlap(pad, r) for r in reserved):
                parking = pad
    return path, parking


def _hedge(plot: list[Pt]) -> list[Pt] | None:
    """A hedge line along the longest plot edge, 0.8 m inside."""
    best, best_len = None, 0.0
    c = geom.centroid(plot)
    n = len(plot)
    for i in range(n):
        a, b = plot[i], plot[(i + 1) % n]
        d = math.hypot(b[0] - a[0], b[1] - a[1])
        if d > best_len:
            best, best_len = (a, b), d
    if best is None or best_len < 6.0:
        return None
    a, b = best
    ux, uy = (b[0] - a[0]) / best_len, (b[1] - a[1]) / best_len
    nx, ny = -uy, ux
    if (c[0] - a[0]) * nx + (c[1] - a[1]) * ny < 0:
        nx, ny = -nx, -ny
    inset = 0.8
    p0 = (a[0] + ux * 1.0 + nx * inset, a[1] + uy * 1.0 + ny * inset)
    p1 = (b[0] - ux * 1.0 + nx * inset, b[1] - uy * 1.0 + ny * inset)
    if not (geom.point_in_poly(p0, plot) and geom.point_in_poly(p1, plot)):
        return None
    return [p0, p1]


def _pick_points(lay: Layout, footprint: list[Pt], count: int, clearance: float, spread: float) -> list[Pt]:
    """Greedy: points on a 1 m grid in the plot that keep `clearance` from the building, the reserved
    areas, the plot border and the points picked so far. Each pick maximises its distance to them."""
    min_x, min_y, max_x, max_y = geom.bbox(lay.plot)
    taken = list(lay.trees)
    points: list[Pt] = []
    grid = []
    y = min_y
    while y <= max_y:
        x = min_x
        while x <= max_x:
            grid.append((x, y))
            x += 1.0
        y += 1.0
    for _ in range(count):
        best, best_d = None, 0.0
        for p in grid:
            if not geom.point_in_poly(p, lay.plot) or geom.dist_point_ring(p, lay.plot) < clearance:
                continue
            if geom.point_in_poly(p, footprint) or geom.dist_point_ring(p, footprint) < clearance:
                continue
            if any(geom.point_in_poly(p, r) or geom.dist_point_ring(p, r) < clearance * 0.6 for r in lay.reserved):
                continue
            if lay.hedge and geom.dist_point_segment(p, lay.hedge[0], lay.hedge[1]) < clearance:
                continue
            d = min([math.hypot(p[0] - q[0], p[1] - q[1]) for q in taken + points] or [99.0])
            if d < spread:
                continue
            # prefer points near the building: score is the distance to others, capped
            near = geom.dist_point_ring(p, footprint)
            score = min(d, 12.0) - 0.15 * near
            if best is None or score > best_d:
                best, best_d = p, score
        if best is None:
            break
        points.append(best)
    return points
