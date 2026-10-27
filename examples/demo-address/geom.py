"""Small 2D geometry helpers for the address demo. Pure Python, metres, no dependencies."""

from __future__ import annotations

import math

Pt = tuple[float, float]


def signed_area(ring: list[Pt]) -> float:
    total = 0.0
    for i, (x1, y1) in enumerate(ring):
        x2, y2 = ring[(i + 1) % len(ring)]
        total += x1 * y2 - x2 * y1
    return total / 2.0


def ccw(ring: list[Pt]) -> list[Pt]:
    """The ring in counterclockwise order."""
    return list(ring) if signed_area(ring) >= 0 else list(reversed(ring))


def centroid(ring: list[Pt]) -> Pt:
    area = signed_area(ring)
    if abs(area) < 1e-9:
        return (sum(p[0] for p in ring) / len(ring), sum(p[1] for p in ring) / len(ring))
    cx = cy = 0.0
    for i, (x1, y1) in enumerate(ring):
        x2, y2 = ring[(i + 1) % len(ring)]
        cross = x1 * y2 - x2 * y1
        cx += (x1 + x2) * cross
        cy += (y1 + y2) * cross
    return (cx / (6 * area), cy / (6 * area))


def bbox(ring: list[Pt]) -> tuple[float, float, float, float]:
    xs = [p[0] for p in ring]
    ys = [p[1] for p in ring]
    return min(xs), min(ys), max(xs), max(ys)


def point_in_poly(p: Pt, ring: list[Pt]) -> bool:
    x, y = p
    inside = False
    n = len(ring)
    for i in range(n):
        x1, y1 = ring[i]
        x2, y2 = ring[(i + 1) % n]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            inside = not inside
    return inside


def dist_point_segment(p: Pt, a: Pt, b: Pt) -> float:
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    length2 = dx * dx + dy * dy
    t = 0.0 if length2 == 0 else max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / length2))
    return math.hypot(p[0] - (ax + t * dx), p[1] - (ay + t * dy))


def dist_point_ring(p: Pt, ring: list[Pt]) -> float:
    """Distance to the outline of a ring."""
    return min(dist_point_segment(p, ring[i], ring[(i + 1) % len(ring)]) for i in range(len(ring)))


def min_rect(ring: list[Pt]) -> dict:
    """The smallest rotated rectangle around a ring (turns the ring along each of its edges).

    Returns {origin, u, v, lo_u, hi_u, lo_v, hi_v, length, width, corners}. u is the unit
    vector along the longer side, v is u turned 90 degrees counterclockwise."""
    best = None
    n = len(ring)
    for i in range(n):
        x1, y1 = ring[i]
        x2, y2 = ring[(i + 1) % n]
        d = math.hypot(x2 - x1, y2 - y1)
        if d < 1e-6:
            continue
        u = ((x2 - x1) / d, (y2 - y1) / d)
        v = (-u[1], u[0])
        us = [p[0] * u[0] + p[1] * u[1] for p in ring]
        vs = [p[0] * v[0] + p[1] * v[1] for p in ring]
        area = (max(us) - min(us)) * (max(vs) - min(vs))
        if best is None or area < best[0] - 1e-9:
            best = (area, u, v, min(us), max(us), min(vs), max(vs))
    if best is None:
        raise ValueError("ring has no edge")
    _, u, v, lo_u, hi_u, lo_v, hi_v = best
    if (hi_v - lo_v) > (hi_u - lo_u):  # make u the long side
        u, v = v, (-u[0], -u[1])
        us = [p[0] * u[0] + p[1] * u[1] for p in ring]
        vs = [p[0] * v[0] + p[1] * v[1] for p in ring]
        lo_u, hi_u, lo_v, hi_v = min(us), max(us), min(vs), max(vs)

    def at(a: float, b: float) -> Pt:
        return (a * u[0] + b * v[0], a * u[1] + b * v[1])

    return {"u": u, "v": v, "lo_u": lo_u, "hi_u": hi_u, "lo_v": lo_v, "hi_v": hi_v,
            "length": hi_u - lo_u, "width": hi_v - lo_v, "at": at,
            "corners": [at(lo_u, lo_v), at(hi_u, lo_v), at(hi_u, hi_v), at(lo_u, hi_v)]}


def rect_in_frame(frame: dict, a0: float, a1: float, b0: float, b1: float) -> list[Pt]:
    """A rectangle given in the (u, v) frame of `min_rect`, as four corners counterclockwise."""
    at = frame["at"]
    return [at(a0, b0), at(a1, b0), at(a1, b1), at(a0, b1)]


def expand_rect(frame: dict, margin: float) -> list[Pt]:
    return rect_in_frame(frame, frame["lo_u"] - margin, frame["hi_u"] + margin,
                         frame["lo_v"] - margin, frame["hi_v"] + margin)


def free_space(frame: dict, plot: list[Pt], limit: float = 60.0, step: float = 0.5) -> dict[str, float]:
    """The free distance from each side of the frame rectangle to the plot border.

    Sides: 'u+', 'u-', 'v+', 'v-'. The distance is measured from the middle of the side straight
    out, and capped at `limit`. A side that starts outside the plot has 0."""
    mu = (frame["lo_u"] + frame["hi_u"]) / 2
    mv = (frame["lo_v"] + frame["hi_v"]) / 2
    at = frame["at"]
    probes = {"u+": (frame["hi_u"], mv, 1, 0), "u-": (frame["lo_u"], mv, -1, 0),
              "v+": (mu, frame["hi_v"], 0, 1), "v-": (mu, frame["lo_v"], 0, -1)}
    out = {}
    for side, (a, b, da, db) in probes.items():
        d = 0.0
        while d < limit and point_in_poly(at(a + da * (d + step), b + db * (d + step)), plot):
            d += step
        out[side] = d
    return out


def inset_ok(rect: list[Pt], plot: list[Pt], margin: float = 0.0) -> bool:
    """True when every corner of the rectangle lies in the plot, at least `margin` from its border."""
    for p in rect:
        if not point_in_poly(p, plot) or dist_point_ring(p, plot) < margin:
            return False
    return True


def rects_overlap(a: list[Pt], b: list[Pt]) -> bool:
    """Overlap test for two convex polygons by the separating axis theorem. Touching is no overlap."""
    for poly in (a, b):
        for i in range(len(poly)):
            x1, y1 = poly[i]
            x2, y2 = poly[(i + 1) % len(poly)]
            nx, ny = y1 - y2, x2 - x1
            pa = [p[0] * nx + p[1] * ny for p in a]
            pb = [p[0] * nx + p[1] * ny for p in b]
            if max(pa) <= min(pb) + 1e-9 or max(pb) <= min(pa) + 1e-9:
                return False
    return True
