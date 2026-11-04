"""Sample values for message variables, chosen by the variable name."""
import re

NUMERIC = {
    "n": "2", "count": "2", "total": "5", "used": "2", "free": "2", "blocked": "1", "step": "2",
    "index": "2", "id": "7", "num": "2", "number": "2", "px": "16", "mm": "250", "m": "12.5", "area": "42",
    "width": "12.5", "height": "12.5", "size": "12.5", "length": "12.5", "radius": "12.5", "percent": "80",
    "pct": "80", "depth": "12.5", "diameter": "12.5", "scale": "12.5", "angle": "45", "x": "12.5", "y": "12.5",
}
TEXT = {
    "file": "plan.kad", "path": "plan.kad", "filename": "plan.kad", "name": "Näidis", "names": "Näidis, Hekk",
    "version": "0.1.0", "error": "Permission denied", "reason": "Permission denied", "e": "Permission denied",
    "key": "Ctrl+Z", "channel": "alpha", "kind": "line", "layer": "Kiht 1", "signature": "a1b2c3",
    "title": "Näidis", "label": "Näidis", "tag": "Näidis", "sheet": "Leht 1", "plant": "Näidis",
}
NUM_RE = re.compile(r"^-?\d+(?:[.,]\d+)?$")


def default_sample(name):
    """Sample value text for a variable name. Unknown names give the name in brackets."""
    low = name.lower()
    if low in NUMERIC:
        return NUMERIC[low]
    if low in TEXT:
        return TEXT[low]
    for suffix, table in (("count", NUMERIC), ("total", NUMERIC), ("width", NUMERIC), ("height", NUMERIC),
                          ("size", NUMERIC), ("length", NUMERIC), ("name", TEXT), ("path", TEXT),
                          ("file", TEXT), ("error", TEXT)):
        if low.endswith(suffix) and low != suffix:
            return table[suffix]
    return "[%s]" % name


def sample_for(name, owner):
    """The owner's value when set, else the default."""
    v = (owner or {}).get(name)
    return v if isinstance(v, str) and v != "" else default_sample(name)
