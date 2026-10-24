"""Scenario: a DXF file has block references with a clip (XCLIP) and Kadakas cuts the geometry (IOP-27).

The files are made with ezdxf (`ezdxf.xclip.XClip`). The cut length of the lines is compared with the
length that shapely finds for the same geometry and the same polygon in the drawing.
"""

import pytest
from pytest import approx

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf --with shapely pytest tests/behavior")
shapely_geometry = pytest.importorskip("shapely.geometry", reason="run with: uv run --with pytest --with ezdxf --with shapely pytest tests/behavior")
from ezdxf.xclip import XClip  # noqa: E402
from shapely.geometry import LineString, Polygon  # noqa: E402

CLIP = [(0, 0), (60, 0), (60, 20), (40, 20), (40, 8), (20, 8), (20, 20), (0, 20)]


def make_doc(path, placements, clip=CLIP, invert=False):
    doc = ezdxf.new("R2018")
    doc.units = 4
    block = doc.blocks.new("PLAN")
    block.add_line((0, 1), (80, 1))
    block.add_line((0, 10), (80, 10))
    block.add_lwpolyline([(5, 2), (70, 2), (70, 18), (5, 18)], close=True)
    block.add_line((200, 0), (210, 0))
    block.add_text("TAG", dxfattribs={"insert": (10, 5), "height": 3})
    block.add_text("FAR", dxfattribs={"insert": (300, 5), "height": 3})
    msp = doc.modelspace()
    for at, rotation, scale in placements:
        ins = msp.add_blockref("PLAN", at, dxfattribs={"rotation": rotation, "xscale": scale, "yscale": scale, "zscale": scale})
        xc = XClip(ins)
        xc.set_wcs_clipping_path([ins.matrix44().transform(ezdxf.math.Vec3(p)) for p in clip])
        if invert:
            xc.invert_clipping_path(ignore_acad_compatibility=True)
    doc.saveas(str(path))
    return doc


def expected_length(doc):
    total = 0.0
    for ins in doc.modelspace().query("INSERT"):
        wcs = XClip(ins).get_wcs_clipping_path()
        region = Polygon([(v.x, v.y) for v in wcs.vertices]).buffer(0)
        if wcs.is_inverted_clip and wcs.inverted_clip:
            region = region.difference(Polygon([(v.x, v.y) for v in wcs.inverted_clip]).buffer(0))
        for e in ins.virtual_entities():
            if e.dxftype() == "LINE":
                line = LineString([(e.dxf.start.x, e.dxf.start.y), (e.dxf.end.x, e.dxf.end.y)])
            elif e.dxftype() == "LWPOLYLINE":
                pts = [(p[0], p[1]) for p in e.get_points("xy")]
                line = LineString(pts + [pts[0]] if e.closed else pts)
            else:
                continue
            total += line.intersection(region).length
    return total


def drawn_length(cad):
    total = 0.0
    for o in cad.result("objects.list"):
        s = o["shape"]
        if s["kind"] == "line":
            (ax, ay), (bx, by) = s["a"], s["b"]
            total += ((ax - bx) ** 2 + (ay - by) ** 2) ** 0.5
        elif s["kind"] == "polyline":
            pts = [v if isinstance(v, list) else v["p"] for v in s["points"]]
            if s.get("closed"):
                pts = pts + [pts[0]]
            total += sum(((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5 for a, b in zip(pts, pts[1:]))
    return total


def texts(cad):
    return sorted(o["shape"]["text"] for o in cad.result("objects.list") if o["shape"]["kind"] == "text")


@pytest.mark.parametrize(
    "placements",
    [
        [((100, 100), 0, 1)],
        [((100, 100), 30, 1), ((300, 100), 90, 2)],
        [((0, 0), 0, 1), ((50, 50), 45, 0.5), ((-40, 30), 180, 1.5)],
    ],
)
def test_the_cut_length_equals_the_length_of_shapely(cad, tmp_path, placements):
    path = tmp_path / "x.dxf"
    doc = make_doc(path, placements)
    r = cad.result("import.file", {"path": str(path), "options": {"unit": "mm", "tolerance": 0.01}})["report"]
    assert r["entities_read"] == r["entities_imported"] + r.get("entities_skipped", 0)
    assert drawn_length(cad) == approx(expected_length(doc), rel=1e-3)
    n = len(placements)
    assert any(f"{n} block reference" in note and "was applied" in note for note in r["notes"]), r["notes"]
    # The line and the text that lie far outside the clip are not imported, and the report says why.
    lost = sum(s["count"] for s in r["skipped"] if "outside the clip (XCLIP)" in s["reason"])
    assert lost >= 2 * n, r["skipped"]
    assert texts(cad) == ["TAG"] * n


def test_a_line_on_the_edge_of_the_clip_stays(cad, tmp_path):
    """A frame that lies on the edge of the clip is not cut away (the unit tests cover the details)."""
    doc = ezdxf.new("R2018")
    doc.units = 4
    block = doc.blocks.new("FRAME")
    block.add_lwpolyline([(0, 0), (50, 0), (50, 30), (0, 30)], close=True)
    ins = doc.modelspace().add_blockref("FRAME", (10, 10))
    XClip(ins).set_wcs_clipping_path([(10, 10), (60, 40)])
    path = tmp_path / "frame.dxf"
    doc.saveas(str(path))
    cad.result("import.file", {"path": str(path), "options": {"unit": "mm"}})
    assert drawn_length(cad) == approx(160.0, rel=1e-6)


def test_an_inverted_clip_is_cut_the_other_way(cad, tmp_path):
    path = tmp_path / "x.dxf"
    doc = make_doc(path, [((100, 100), 0, 1)], clip=[(0, 0), (70, 0), (70, 20), (0, 20)], invert=True)
    cad.result("import.file", {"path": str(path), "options": {"unit": "mm", "tolerance": 0.01}})
    assert drawn_length(cad) == approx(expected_length(doc), rel=1e-3)
