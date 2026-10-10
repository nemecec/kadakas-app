"""Helpers for writing plug-in object scripts, and a way to test a script without the app.

Inside the app, the helpers ``line``, ``rect``, ``circle``, ``arc``, ``polyline``, ``text``,
``rotate``, ``walk`` and ``path_length`` are already available to a script. Import them from this
module to run the same script in your own Python::

    from kadakas.plugin import *
    shapes = test_reset(open("bench.py").read(), {"length": 1800, ...}, {"at": [0, 0], "rotation": 0})
"""

from __future__ import annotations

from typing import Any

from .plugin_runner import (  # noqa: F401 - re-exported for scripts
    HELPERS,
    AttrDict,
    arc,
    circle,
    line,
    path_length,
    polyline,
    rect,
    rotate,
    run_reset,
    text,
    walk,
)

__all__ = [
    "AttrDict", "arc", "circle", "line", "path_length", "polyline", "rect", "rotate", "test_reset",
    "text", "walk",
]


def test_reset(script: str, params: dict[str, Any], geometry: dict[str, Any],
               name: str = "plug-in") -> list[dict[str, Any]]:
    """Run ``reset(params, geometry)`` of a script in this process, as the app does.
    Returns the shapes, or raises ``RuntimeError`` with the error of the script.
    ``test_reset.__test__`` is false, so pytest does not collect it."""
    answer = run_reset({"script": script, "params": params, "geometry": geometry, "name": name})
    if not answer["ok"]:
        raise RuntimeError(answer["error"] + "\n" + answer.get("traceback", ""))
    return answer["shapes"]


test_reset.__test__ = False  # type: ignore[attr-defined]
