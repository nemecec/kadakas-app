"""Scenario: an agent or a script looks at the drawing as SVG, through `render.svg` and `kadakas render`."""

import pytest

from helpers import parse_svg, plant_cmd, rect_cmd, run_cli, svg_object_ids

MIXED_SHAPES = [
    {"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}},
    {"op": "add_object", "shape": {"kind": "circle", "center": [500, 500], "radius": 300}},
    {"op": "add_object", "shape": {"kind": "arc", "center": [0, 0], "radius": 500, "start": 0, "end": 1.57}},
    {"op": "add_object", "shape": {"kind": "polyline", "points": [[0, 0], [10, 10], [20, 0]], "closed": False}},
    {"op": "add_object", "shape": {"kind": "polyline", "points": [[0, 0], [10, 10], [20, 0]], "closed": True}},
    {"op": "add_object", "shape": {"kind": "text", "at": [0, 2000], "text": "A & B <tag>", "height": 100}},
    {"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [3000, 0], [3000, 1200]], "material": "Setts"}},
    {"op": "add_object", "shape": {"kind": "landscape_area", "boundary": [[0, 0], [5, 0], [5, 5]], "name": "Meadow"}},
    plant_cmd(2000, 2000, species="Acer <campestre>"),
]


def build_mixed_drawing(cad):
    """Nine shapes on the default layer, plus one more shape in a class that is then hidden."""
    for command in MIXED_SHAPES:
        cad.execute(command)
    hidden_class = cad.execute({"op": "add_class", "name": "Hidden"})[0]
    cad.execute(rect_cmd(-100, -100, -50, -50, **{"class": hidden_class}))
    cad.execute({"op": "modify_class", "id": hidden_class, "visible": False})
    return len(MIXED_SHAPES)


@pytest.mark.parametrize(("requested", "expected"), [(400, 400), (16, 16), (0, 16), (100000, 8192)])
def test_render_svg_clamps_width_to_a_sane_range(cad, requested, expected):
    cad.execute(rect_cmd(0, 0, 10, 10))
    root = parse_svg(cad.result("render.svg", {"width": requested})["svg"])
    assert root.get("width") == str(expected)


def test_render_svg_default_width_is_800(cad):
    cad.execute(rect_cmd(0, 0, 10, 10))
    root = parse_svg(cad.result("render.svg")["svg"])
    assert root.get("width") == "800"


def test_render_svg_has_one_element_per_visible_object(cad):
    visible = build_mixed_drawing(cad)
    svg = cad.result("render.svg", {"width": 600})["svg"]
    ids = svg_object_ids(svg)
    assert len(ids) == visible
    assert len(set(ids)) == visible
    assert len(cad.result("objects.list")) == visible + 1, "the hidden object still exists"


def test_render_svg_of_empty_drawing_is_valid_and_empty(cad):
    root = parse_svg(cad.result("render.svg")["svg"])
    assert svg_object_ids(cad.result("render.svg")["svg"]) == []
    assert root.tag.endswith("svg")


def test_render_cli_writes_valid_svg_matching_visible_objects(make_serve, tmp_path):
    doc_path = tmp_path / "garden.kad"
    svg_path = tmp_path / "garden.svg"
    cad = make_serve()
    visible = build_mixed_drawing(cad)
    cad.result("file.save", {"path": str(doc_path)})

    proc = run_cli("render", str(doc_path), "-o", str(svg_path))
    assert proc.returncode == 0, proc.stderr
    text = svg_path.read_text()
    assert len(svg_object_ids(text)) == visible
    assert parse_svg(text).get("width") == "1200", "the CLI default width is 1200 px"


def test_render_cli_width_flag_is_honored(make_serve, tmp_path):
    doc_path = tmp_path / "d.kad"
    svg_path = tmp_path / "d.svg"
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("file.save", {"path": str(doc_path)})
    proc = run_cli("render", str(doc_path), "-o", str(svg_path), "--width", "640")
    assert proc.returncode == 0, proc.stderr
    assert parse_svg(svg_path.read_text()).get("width") == "640"


def test_render_cli_fails_cleanly_for_missing_input(tmp_path):
    proc = run_cli("render", str(tmp_path / "nope.kad"), "-o", str(tmp_path / "out.svg"))
    assert proc.returncode != 0
    assert "nope.kad" in proc.stderr
    assert not (tmp_path / "out.svg").exists()


def test_render_cli_fails_cleanly_for_invalid_document(tmp_path):
    bad = tmp_path / "bad.kad"
    bad.write_text("{not a document")
    proc = run_cli("render", str(bad), "-o", str(tmp_path / "out.svg"))
    assert proc.returncode != 0
    assert "bad.kad" in proc.stderr
