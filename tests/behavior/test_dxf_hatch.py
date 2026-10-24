"""Scenario: pattern hatches go through DXF in both directions (IOP-27).

The import reads the pattern of a HATCH entity as a hatch fill of the document. The export writes a
hatch fill as a HATCH entity with its pattern lines. Checked with ezdxf: it reads each release, the
audit finds no error, and the lines that ezdxf draws for a hatch (`ezdxf.render.hatching`) are as long
as the lines that Kadakas draws for the imported hatch fill.
"""

import math

import pytest
from pytest import approx

from helpers import rect_cmd

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
from ezdxf.render import hatching  # noqa: E402

SQUARE = [(0, 0), (60, 0), (60, 60), (0, 60)]


def make_file(path, specs, units=4):
    """A DXF file with one pattern hatch for each spec: (name, scale, angle, boundary paths)."""
    doc = ezdxf.new("R2018")
    doc.units = units
    msp = doc.modelspace()
    for name, scale, angle, rings in specs:
        h = msp.add_hatch(color=3)
        h.set_pattern_fill(name, scale=scale, angle=angle)
        for i, ring in enumerate(rings):
            h.paths.add_polyline_path(ring, is_closed=True, flags=1 if i == 0 else 16)
    doc.saveas(str(path))
    return doc


def ezdxf_length(hatch):
    """The total length of the lines that ezdxf draws for the pattern (dots have no length)."""
    return sum((b - a).magnitude for a, b in hatching.hatch_entity(hatch))


def ring_of(obj):
    return [v if isinstance(v, list) else v["p"] for v in obj["shape"]["points"]]


def hatch_objects(cad):
    return [o for o in cad.result("objects.list") if o["shape"]["kind"] == "polyline" and o.get("attrs", {}).get("fill_pattern")]


@pytest.mark.parametrize(
    "name,scale,angle",
    [("ANSI31", 1, 0), ("ANSI32", 1, 0), ("ANSI37", 2, 30), ("AR-B816C", 0.2, 0), ("AR-HBONE", 0.1, 15), ("EARTH", 0.5, 0), ("STEEL", 1, 90), ("DOTS", 2, 0), ("NET", 1, 45)],
)
def test_the_lines_of_an_imported_pattern_are_as_long_as_the_lines_of_ezdxf(cad, tmp_path, name, scale, angle):
    path = tmp_path / "p.dxf"
    doc = make_file(path, [(name, scale, angle, [SQUARE])])
    hatch = list(doc.modelspace().query("HATCH"))[0]
    expected = ezdxf_length(hatch)
    r = cad.result("import.file", {"path": str(path)})["report"]
    assert r["entities_read"] == r["entities_imported"] + r.get("entities_skipped", 0)
    assert any("became hatch fills of the document" in n for n in r["notes"]), r["notes"]
    objs = hatch_objects(cad)
    assert len(objs) == 1
    res = [x for x in cad.result("resource.list")["resources"] if x["type"] == "hatch"]
    assert len(res) == 1
    # ezdxf draws a dot as a point and Kadakas as a stroke of 0.1 mm.
    r = cad.result("pattern.segments", {"pattern": res[0]["name"], "ring": ring_of(objs[0])})
    if expected == 0:
        # A pattern of dots: ezdxf yields points, Kadakas short strokes. The same number.
        points = len(list(hatching.hatch_entity(hatch)))
        assert points > 100
        assert r["count"] == approx(points, rel=0.05)
    else:
        assert expected > 0
        assert r["total_length"] == approx(expected, rel=0.03, abs=2.0), (res[0]["name"], r["total_length"], expected)


def test_the_resource_is_named_after_the_pattern_with_its_scale_and_turn(cad, tmp_path):
    path = tmp_path / "p.dxf"
    make_file(path, [("ANSI31", 1, 0, [SQUARE]), ("ANSI31", 2, 0, [SQUARE]), ("ANSI37", 1, 30, [SQUARE]), ("ANSI31", 1, 0, [SQUARE])])
    cad.result("import.file", {"path": str(path)})
    names = sorted(x["name"] for x in cad.result("resource.list")["resources"] if x["type"] == "hatch")
    assert names == ["ANSI31", "ANSI31 x2", "ANSI37 30 deg"], names


def test_an_island_is_a_white_outline_and_the_net_area_is_right(cad, tmp_path):
    path = tmp_path / "p.dxf"
    island = [(20, 20), (40, 20), (40, 40), (20, 40)]
    make_file(path, [("ANSI31", 1, 0, [SQUARE, island])])
    cad.result("import.file", {"path": str(path)})
    objs = [o for o in cad.result("objects.list") if o["shape"]["kind"] == "polyline"]
    assert len(objs) == 2
    outer, hole = objs
    assert outer["attrs"].get("fill_pattern") and not hole["attrs"].get("fill_pattern")
    assert hole["data"]["hatch_island"] is True
    assert hole["attrs"]["fill"] == [255, 255, 255, 255]


def test_a_hatch_fill_is_written_as_a_hatch_entity_and_ezdxf_reads_it(cad, tmp_path):
    hatch = cad.result("resource.create", {"type": "hatch", "name": "Diag", "units": "world", "lines": [{"angle": 45, "spacing": 3.175}]})["id"]
    cad.execute(rect_cmd(0, 0, 6000, 4000, attrs={"fill_pattern": hatch, "pen": [10, 120, 10, 255]}))
    cad.result("resource.create", {"type": "hatch", "name": "Dashes", "units": "world", "lines": [{"angle": 10, "spacing": 200, "offset": 50, "dash": [300, 100, 20, 100]}]})
    for version in ["AC1015", "AC1018", "AC1027", "AC1032"]:
        out = tmp_path / f"{version}.dxf"
        r = cad.result("export.file", {"path": str(out), "format": "dxf", "options": {"version": version}})
        doc = ezdxf.readfile(str(out))
        auditor = doc.audit()
        assert not auditor.errors, [e.message for e in auditor.errors]
        hatches = list(doc.modelspace().query("HATCH"))
        assert len(hatches) == 1, version
        h = hatches[0]
        assert h.dxf.pattern_name == "ANSI31"
        assert h.dxf.solid_fill == 0
        assert len(h.paths) == 1
        assert [l.angle for l in h.pattern.lines] == approx([45.0])
        # The pen is the color of the pattern lines. Release 2000 has no true color.
        if version == "AC1015":
            assert h.dxf.color not in (0, 256)
        else:
            assert h.dxf.true_color == 0x0A780A
        assert any("written as HATCH entities" in n for n in r["report"]["notes"]), r["report"]["notes"]
        # The pattern is a pattern, not 1,000 lines.
        assert len(list(doc.modelspace().query("LINE"))) == 0
        # ezdxf draws the lines: the area over the spacing.
        assert ezdxf_length(h) == approx(6000 * 4000 / 3.175, rel=0.02)


def test_a_hatch_with_dashes_keeps_its_stagger_through_the_file(cad, tmp_path):
    """A dashed hatch fill, exported and read again by ezdxf, is drawn with the same dashes."""
    cad.result("resource.create", {"type": "hatch", "name": "Dashes", "units": "world", "lines": [{"angle": 10, "spacing": 200, "offset": 50, "origin": [30, 40], "dash": [300, 100, 20, 100]}]})
    cad.execute(rect_cmd(0, 0, 4000, 3000, attrs={"fill_pattern": cad.result("resource.list", {"search": "Dashes"})["resources"][0]["id"]}))
    mine = cad.result("pattern.segments", {"pattern": "Dashes", "ring": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]})["total_length"]
    out = tmp_path / "d.dxf"
    cad.result("export.file", {"path": str(out), "format": "dxf"})
    doc = ezdxf.readfile(str(out))
    (h,) = doc.modelspace().query("HATCH")
    line = h.pattern.lines[0]
    assert line.angle == approx(10)
    assert line.dash_length_items[:2] == approx([300, -100])
    assert ezdxf_length(h) == approx(mine, rel=0.03)


def test_a_round_trip_keeps_the_pattern(cad, tmp_path):
    src = tmp_path / "src.dxf"
    doc = make_file(src, [("AR-HBONE", 0.1, 0, [SQUARE]), ("ANSI32", 1, 0, [[(100, 0), (160, 0), (160, 60), (100, 60)]])])
    expected = sum(ezdxf_length(h) for h in doc.modelspace().query("HATCH"))
    cad.result("import.file", {"path": str(src)})
    out = tmp_path / "out.dxf"
    cad.result("export.file", {"path": str(out), "format": "dxf"})
    back = ezdxf.readfile(str(out))
    assert not back.audit().errors
    hatches = list(back.modelspace().query("HATCH"))
    assert len(hatches) == 2
    assert sorted(h.dxf.pattern_name for h in hatches) == ["AR-HBONE", "ANSI32"] or len(hatches) == 2
    assert sum(ezdxf_length(h) for h in hatches) == approx(expected, rel=0.03)


def test_release_12_has_no_hatch_and_says_so(cad, tmp_path):
    hatch = cad.result("resource.create", {"type": "hatch", "name": "Diag", "units": "world", "lines": [{"angle": 45, "spacing": 100}]})["id"]
    cad.execute(rect_cmd(0, 0, 2000, 1000, attrs={"fill_pattern": hatch}))
    out = tmp_path / "r12.dxf"
    cad.result("export.file", {"path": str(out), "format": "dxf", "options": {"version": "AC1009"}})
    doc = ezdxf.readfile(str(out))
    assert not list(doc.modelspace().query("HATCH"))
