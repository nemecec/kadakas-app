"""Runs one reset of a plug-in object. The app starts this file with ``python3 -I -c``.

Input (JSON on stdin)::

    {"event": "reset", "name": "Bench", "shape_type": "point",
     "params": {...}, "geometry": {...}, "units": "mm", "script": "<python source>"}

Output (one JSON line on stdout)::

    {"ok": true, "shapes": [...]}  or  {"ok": false, "error": "...", "traceback": "..."}

The script must define ``reset(params, geometry)`` and return a list of shapes. Anything the script
prints goes to standard error, so it can never break the answer. This file uses only the standard
library. It also holds the helper functions that the script can call without an import.
"""

from __future__ import annotations

import json
import math
import sys
import traceback


class AttrDict(dict):
    """A dict whose keys are also attributes: ``params.width`` is ``params["width"]``."""

    def __getattr__(self, key):
        try:
            value = self[key]
        except KeyError:
            raise AttributeError(key) from None
        return AttrDict(value) if isinstance(value, dict) else value


def _color(value):
    """A color as ``#rrggbb`` or ``#rrggbbaa`` text, or as a list of 3 or 4 numbers."""
    if isinstance(value, str):
        h = value.lstrip("#")
        if len(h) not in (6, 8):
            raise ValueError(f"color {value!r} must look like #rrggbb")
        parts = [int(h[i : i + 2], 16) for i in range(0, len(h), 2)]
    else:
        parts = [int(v) for v in value]
    if len(parts) == 3:
        parts.append(255)
    return parts


def _attrs(shape: dict, pen=None, fill=None, weight=None) -> dict:
    attrs = {}
    if pen is not None:
        attrs["pen"] = _color(pen)
    if fill is not None:
        attrs["fill"] = _color(fill)
    if weight is not None:
        attrs["line_weight"] = float(weight)
    if attrs:
        shape["attrs"] = attrs
    return shape


def _pt(p):
    return [float(p[0]), float(p[1])]


def line(a, b, *, pen=None, weight=None):
    """A straight line from ``a`` to ``b`` (each is ``[x, y]`` in mm)."""
    return _attrs({"kind": "line", "a": _pt(a), "b": _pt(b)}, pen=pen, weight=weight)


def rect(min, max, *, pen=None, fill=None, weight=None):
    """A rectangle with the corners ``min`` and ``max``, parallel to the axes."""
    shape = {"kind": "rect", "min": _pt(min), "max": _pt(max)}
    return _attrs(shape, pen=pen, fill=fill, weight=weight)


def circle(center, radius, *, pen=None, fill=None, weight=None):
    """A circle."""
    shape = {"kind": "circle", "center": _pt(center), "radius": float(radius)}
    return _attrs(shape, pen=pen, fill=fill, weight=weight)


def arc(center, radius, start, end, *, pen=None, weight=None):
    """A counterclockwise arc. Angles are in radians."""
    shape = {
        "kind": "arc",
        "center": _pt(center),
        "radius": float(radius),
        "start": float(start),
        "end": float(end),
    }
    return _attrs(shape, pen=pen, weight=weight)


def polyline(points, closed=False, *, pen=None, fill=None, weight=None):
    """A polyline through the points. ``closed=True`` joins the last point to the first."""
    shape = {"kind": "polyline", "points": [_pt(p) for p in points], "closed": bool(closed)}
    return _attrs(shape, pen=pen, fill=fill, weight=weight)


def text(at, text, height=200.0, *, pen=None):
    """A text label at a point. ``height`` is the text height in mm."""
    shape = {"kind": "text", "at": _pt(at), "text": str(text), "height": float(height)}
    return _attrs(shape, pen=pen)


def rotate(p, angle, about=(0.0, 0.0)):
    """The point ``p`` turned counterclockwise by ``angle`` radians around ``about``."""
    c, s = math.cos(angle), math.sin(angle)
    x, y = p[0] - about[0], p[1] - about[1]
    return [about[0] + x * c - y * s, about[1] + x * s + y * c]


def path_length(points, closed=False):
    """The length of a path in mm."""
    pts = list(points) + ([points[0]] if closed and points else [])
    return sum(math.dist(a, b) for a, b in zip(pts, pts[1:]))


def walk(points, spacing, start=0.0, closed=False):
    """Walks along a path and yields ``(x, y, angle)`` every ``spacing`` mm.

    ``start`` is the distance of the first stop from the start of the path. ``angle`` is the
    direction of the path at the stop, in radians.
    """
    if spacing <= 0:
        raise ValueError("spacing must be greater than zero")
    pts = [tuple(p) for p in points] + ([tuple(points[0])] if closed and points else [])
    target = float(start)
    walked = 0.0
    for a, b in zip(pts, pts[1:]):
        seg = math.dist(a, b)
        if seg == 0:
            continue
        angle = math.atan2(b[1] - a[1], b[0] - a[0])
        while target <= walked + seg + 1e-9:
            t = (target - walked) / seg
            yield (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, angle)
            target += spacing
        walked += seg


HELPERS = {
    "AttrDict": AttrDict,
    "math": math,
    "line": line,
    "rect": rect,
    "circle": circle,
    "arc": arc,
    "polyline": polyline,
    "text": text,
    "rotate": rotate,
    "path_length": path_length,
    "walk": walk,
}


def run_reset(request: dict) -> dict:
    """Runs the script of the request and returns the answer as a dict."""
    namespace = dict(HELPERS, __name__="kadakas_plugin")
    try:
        name = request.get("name", "plug-in")
        code = compile(request["script"], f"<plug-in {name}>", "exec")
        exec(code, namespace)
        reset = namespace.get("reset")
        if not callable(reset):
            raise RuntimeError("the script must define reset(params, geometry)")
        result = reset(AttrDict(request.get("params", {})), AttrDict(request.get("geometry", {})))
        if result is None:
            result = []
        shapes = list(result)
        json.dumps(shapes, allow_nan=False)  # checks that every number is finite
        return {"ok": True, "shapes": shapes}
    except BaseException as e:  # noqa: BLE001 - every failure must become an answer
        tb = traceback.format_exc()
        return {"ok": False, "error": f"{type(e).__name__}: {e}", "traceback": tb[-4000:]}


def main() -> None:
    answer_stream = sys.stdout
    sys.stdout = sys.stderr  # print() in the script must not break the answer
    request = json.load(sys.stdin)
    answer = run_reset(request)
    answer_stream.write(json.dumps(answer, allow_nan=False))
    answer_stream.write("\n")
    answer_stream.flush()


if __name__ == "__main__":
    main()
