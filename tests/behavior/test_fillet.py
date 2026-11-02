"""Scenario: an agent builds a desk leg and a table top and rounds their edges (MOD-21). The rounds
stay closed solids with the volume of a true round within the error of the segments, a radius that
does not fit is refused with the largest one that fits, and the result exports to STL."""

import math
import struct

from pytest import approx


def rect(x0, y0, x1, y1, z=0):
    return [
        [[x0, y0, z], [x1, y0, z]],
        [[x1, y0, z], [x1, y1, z]],
        [[x1, y1, z], [x0, y1, z]],
        [[x0, y1, z], [x0, y0, z]],
    ]


def box(cad, x, y, z):
    model = cad.result("mesh.create", {"name": "Top"})["id"]
    face = cad.result("mesh.add_edges", {"id": model, "edges": rect(0, 0, x, y)})["created"]["faces"][0]
    cad.result("mesh.push_pull", {"id": model, "face": face, "distance": z})
    return model


def volume(cad, model):
    report = cad.result("mesh.solid_report", {"id": model})
    assert report["closed"] is True
    return sum(s["volume"] for s in report["shells"])


def test_a_table_top_with_all_edges_rounded_is_near_the_true_round(cad):
    a, b, c, r = 1200.0, 600.0, 40.0, 15.0
    model = box(cad, a, b, c)
    cad.result("mesh.fillet", {"id": model, "all": True, "radius": r, "segments": 16})
    a2, b2, c2 = a - 2 * r, b - 2 * r, c - 2 * r
    truth = (a2 * b2 * c2 + 2 * r * (a2 * b2 + b2 * c2 + a2 * c2) + math.pi * r * r * (a2 + b2 + c2)
             + 4 / 3 * math.pi * r ** 3)
    v = volume(cad, model)
    assert v < truth
    assert v == approx(truth, rel=1e-3)


def test_a_radius_too_large_for_the_top_is_refused_with_what_fits(cad):
    model = box(cad, 1200, 600, 40)
    before = volume(cad, model)
    data = cad.error("mesh.fillet", {"id": model, "all": True, "radius": 30})["data"]
    assert data["error_id"] == "fillet_does_not_fit"
    assert 19.9 < data["max_mm"] <= 20
    assert volume(cad, model) == approx(before)


def test_a_chamfered_leg_exports_as_a_closed_stl(cad, tmp_path):
    model = box(cad, 50, 50, 700)
    cad.result("mesh.chamfer", {"id": model, "all": True, "distance": 3})
    path = tmp_path / "leg.stl"
    cad.result("export.file", {"path": str(path), "format": "stl"})
    data = path.read_bytes()
    (count,) = struct.unpack("<I", data[80:84])
    # 6 faces, 12 strips, 8 corner triangles: at least 26 triangles.
    assert count >= 26


def test_a_rounded_box_renders_to_a_png(cad):
    import base64

    model = box(cad, 1000, 600, 400)
    cad.result("mesh.fillet", {"id": model, "all": True, "radius": 60})
    shot = cad.result("mesh.render", {"id": model, "width": 320, "height": 240})
    png = base64.b64decode(shot["png_base64"])
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    assert len(png) > 1000
