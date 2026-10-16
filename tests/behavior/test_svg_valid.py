"""Scenario: every SVG that the API writes is well-formed XML. A document with every object kind
is drawn in each way the API draws SVG, and an XML parser reads each result. A parser refuses a
repeated attribute, so this finds the whole class of bug where two parts of the code both write
the same attribute (a roof line with two `fill`)."""

from xml.etree import ElementTree

from all_kinds import build_all_kinds


def test_render_svg_of_every_kind_is_valid_xml(cad):
    made = build_all_kinds(cad)
    assert {"massing", "irrigation", "wall", "plant", "site_model", "hardscape"} <= set(made)
    svgs = {"plain": cad.result("render.svg", {"width": 900})["svg"]}
    svgs["narrow"] = cad.result("render.svg", {"width": 40})["svg"]
    for name, svg in svgs.items():
        ElementTree.fromstring(svg)  # raises ParseError on a repeated attribute
    assert 'data-kind="massing_roof"' in svgs["plain"]


def test_roof_line_has_a_single_fill(cad):
    cad.result("massing.add", {"footprint": [[0, 0], [8000, 0], [8000, 5000], [0, 5000]], "height": 3000, "roof": "gable"})
    svg = cad.result("render.svg")["svg"]
    (roof,) = [line for line in svg.splitlines() if "massing_roof" in line]
    assert roof.count(" fill=") == 1
    ElementTree.fromstring(svg)
