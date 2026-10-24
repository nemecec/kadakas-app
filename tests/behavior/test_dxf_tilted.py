"""Scenario: a block reference and a text in a tilted plane come into the drawing (IOP-27).

The block is placed in 3D (lines, polylines, circles become 3D polylines, points move), and a text is
placed flat by its projection. The expected places come from the ezdxf reader: it explodes the block
reference with `virtual_entities`, which applies the extrusion direction with the arbitrary axis
algorithm.
"""

import math

import pytest
from pytest import approx

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
from ezdxf.math import OCS, Vec3  # noqa: E402

NORMAL = Vec3(0.3, -0.5, 0.8).normalize()


def make_doc(path, rotation=25.0, scale=1.5, at=(200, 100, 30)):
    doc = ezdxf.new("R2018")
    doc.units = 4
    block = doc.blocks.new("TILT")
    block.add_line((0, 0), (10, 5))
    block.add_lwpolyline([(1, 1), (6, 2), (8, 7)])
    block.add_point((3, 4, 2))
    block.add_circle((5, 3), 2)
    block.add_text("Tag", dxfattribs={"insert": (2, 1), "height": 1.2, "rotation": 15})
    ins = doc.modelspace().add_blockref(
        "TILT",
        at,
        dxfattribs={"rotation": rotation, "xscale": scale, "yscale": scale, "zscale": scale, "extrusion": NORMAL},
    )
    doc.saveas(str(path))
    return doc, ins


def shapes(cad, kind):
    return [o for o in cad.result("objects.list") if o["shape"]["kind"] == kind]


def close(a, b, tol=1e-6):
    return all(abs(x - y) < tol for x, y in zip(a, b))


def test_a_tilted_block_is_placed_where_ezdxf_puts_it(cad, tmp_path):
    path = tmp_path / "t.dxf"
    doc, ins = make_doc(path)
    virtual = list(ins.virtual_entities())
    r = cad.result("import.file", {"path": str(path), "options": {"unit": "mm", "tolerance": 0.01}})["report"]
    assert r["entities_read"] == r["entities_imported"] + r.get("entities_skipped", 0)
    assert any("1 block reference lies in a tilted plane" in n for n in r["notes"]), r["notes"]
    lines3d = shapes(cad, "polyline3d")

    # The line: its two ends.
    (line,) = [e for e in virtual if e.dxftype() == "LINE"]
    ours = next(o for o in lines3d if len(o["shape"]["points"]) == 2)
    assert close(ours["shape"]["points"][0], tuple(line.dxf.start))
    assert close(ours["shape"]["points"][1], tuple(line.dxf.end))

    # The open polyline: its three corners.
    (poly,) = [e for e in virtual if e.dxftype() == "LWPOLYLINE"]
    expected = [tuple(v) for v in poly.vertices_in_wcs()]
    ours = next(o for o in lines3d if len(o["shape"]["points"]) == 3)
    for got, want in zip(ours["shape"]["points"], expected):
        assert close(got, want)

    # The point.
    (point,) = [e for e in virtual if e.dxftype() == "POINT"]
    (pt,) = shapes(cad, "locus3d")
    assert close(pt["shape"]["p"], tuple(point.dxf.location))

    # The circle: center and radius of the ring of points.
    circles = [e for e in virtual if e.dxftype() in ("CIRCLE", "ELLIPSE")]
    (circle,) = circles
    ring = next(o for o in lines3d if len(o["shape"]["points"]) > 8)["shape"]["points"]
    center = [sum(p[i] for p in ring) / len(ring) for i in range(3)]
    want_center = tuple(circle.ocs().to_wcs(circle.dxf.center)) if circle.dxftype() == "CIRCLE" else tuple(circle.dxf.center)
    assert close(center, want_center, 5e-2)
    radius = circle.dxf.radius if circle.dxftype() == "CIRCLE" else Vec3(circle.dxf.major_axis).magnitude
    assert max(abs(math.dist(p, center) - radius) for p in ring) < 5e-2


def test_a_text_in_a_tilted_block_is_placed_by_its_projection(cad, tmp_path):
    path = tmp_path / "t.dxf"
    doc, ins = make_doc(path)
    (text,) = [e for e in ins.virtual_entities() if e.dxftype() == "TEXT"]
    cad.result("import.file", {"path": str(path), "options": {"unit": "mm", "tolerance": 0.01}})
    (ours,) = shapes(cad, "text")
    ocs = OCS(text.dxf.extrusion)
    at = ocs.to_wcs(text.dxf.insert)
    assert close(ours["shape"]["at"], (at.x, at.y), 1e-6)
    # The baseline and the height as vectors in the world, projected onto the drawing plane.
    turn = math.radians(text.dxf.rotation)
    base = ocs.to_wcs(Vec3(math.cos(turn), math.sin(turn), 0)) - ocs.to_wcs(Vec3(0, 0, 0))
    up = Vec3(text.dxf.extrusion).cross(base).normalize()
    height = text.dxf.height
    expected_height = height * math.hypot(up.x, up.y)
    assert ours["shape"]["height"] == approx(expected_height, rel=1e-6)
    rotation = math.degrees(math.atan2(base.y, base.x)) % 360
    assert ours["data"]["rotation_deg"] == approx(rotation, abs=1e-5)


def test_a_text_in_a_tilted_plane_and_a_report_that_counts_it(cad, tmp_path):
    doc = ezdxf.new("R2018")
    doc.units = 4
    msp = doc.modelspace()
    # Tilted by 60 degrees about X: the projected height is cos(60) = half.
    n = Vec3(0, -math.sin(math.radians(60)), math.cos(math.radians(60)))
    msp.add_text("Slope", dxfattribs={"insert": (10, 20), "height": 10, "extrusion": n})
    # On a wall: full height.
    msp.add_text("Wall", dxfattribs={"insert": (10, 20), "height": 10, "extrusion": (0, -1, 0)})
    path = tmp_path / "t.dxf"
    doc.saveas(str(path))
    r = cad.result("import.file", {"path": str(path), "options": {"unit": "mm"}})["report"]
    assert r["entities_read"] == r["entities_imported"] == 2
    heights = {o["shape"]["text"]: o["shape"]["height"] for o in shapes(cad, "text")}
    assert heights["Slope"] == approx(5.0, rel=1e-6)
    assert heights["Wall"] == approx(10.0, rel=1e-6)
    assert any("2 texts in a tilted plane were placed flat" in note for note in r["notes"]), r["notes"]
    assert any("1 stand in a nearly upright plane" in note for note in r["notes"]), r["notes"]
