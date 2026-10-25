"""Scenario: texture beds (3D-05). A closed shape on the site model gives that part of the terrain
its own texture without changing the grade. An agent draws beds, orders them, and checks the
render and the exports."""

import json
import statistics
import struct

from pytest import approx

from test_render_png import render
from test_site_model import make_site

SQUARE = [[1000, 1000], [7000, 1000], [7000, 7000], [1000, 7000]]


def glb_json(path):
    data = path.read_bytes()
    n = struct.unpack("<I", data[12:16])[0]
    return json.loads(data[20 : 20 + n])


def triangles_by_material(doc):
    """Triangles per material name in a glTF document."""
    names = [m.get("name", "") for m in doc["materials"]]
    out = {}
    for mesh in doc["meshes"]:
        for prim in mesh["primitives"]:
            name = names[prim["material"]]
            out[name] = out.get(name, 0) + doc["accessors"][prim["indices"]]["count"] // 3
    return out


def spread(rows, x0, x1, y0, y1):
    return statistics.pstdev([sum(px[:3]) for row in rows[y0:y1] for px in row[x0:x1]])


def test_a_bed_textures_part_of_the_terrain_and_leaves_the_grade(cad, tmp_path):
    site, _ = make_site(cad)
    cad.result("material.add_defaults")
    before = cad.result("site.cut_fill", {"id": site})
    z = cad.result("site.elevation", {"id": site, "x": 4000, "y": 4000})["z"]
    flat = render(cad, view="top", width=160, height=160)[1]
    bed = cad.result("bed.add", {"boundary": SQUARE, "texture": "Gravel", "tile_size": 300})["id"]
    row = cad.result("bed.get", {"id": bed})
    assert row["texture"] == "Gravel" and row["active"] and row["acts_on"] == [site]
    shown = render(cad, view="top", width=160, height=160)[1]
    # The texture is inside the bed (the middle of the picture), not outside it.
    assert spread(shown, 60, 100, 60, 100) > spread(flat, 60, 100, 60, 100) + 5
    assert spread(shown, 2, 8, 60, 100) == approx(spread(flat, 2, 8, 60, 100), abs=1)
    # The grade did not change.
    assert cad.result("site.cut_fill", {"id": site}) == before
    assert cad.result("site.elevation", {"id": site, "x": 4000, "y": 4000})["z"] == approx(z)
    # The bed is a site modifier object, one undo step.
    obj = cad.result("objects.get", {"id": bed})
    assert obj["shape"]["kind"] == "site_modifier" and obj["shape"]["modifier"]["type"] == "texture_bed"
    assert cad.result("undo")["done"]
    assert cad.result("bed.list")["beds"] == []
    # Exports: image and texture coordinates for the bed.
    cad.result("redo")
    glb = tmp_path / "bed.glb"
    assert cad.result("export.file", {"path": str(glb), "format": "glb"})["report"]["textures"] == 1
    obj_file = tmp_path / "bed.obj"
    cad.result("export.file", {"path": str(obj_file)})
    assert "map_Kd Gravel_0.png" in (tmp_path / "bed.mtl").read_text(encoding="utf-8")
    assert any(line.startswith("vt ") for line in obj_file.read_text(encoding="utf-8").splitlines())


def test_the_front_bed_wins_and_arrange_changes_it(cad, tmp_path):
    make_site(cad)
    cad.result("material.add_defaults")
    back = cad.result("bed.add", {"boundary": SQUARE, "texture": "Grass"})["id"]
    front = cad.result("bed.add", {"boundary": [[4000, 1000], [7500, 1000], [7500, 7000], [4000, 7000]], "texture": "Gravel"})["id"]
    assert [b["id"] for b in cad.result("bed.list")["beds"]] == [back, front]

    def counts(name):
        path = tmp_path / name
        cad.result("export.file", {"path": str(path), "format": "glb"})
        return triangles_by_material(glb_json(path))

    a = counts("a.glb")
    assert a["Gravel"] > 0 and a["Grass"] > 0
    # Overlap area 3000 by 6000 goes to Gravel. Bring Grass to the front: it takes the overlap.
    cad.execute({"op": "arrange", "ids": [back], "how": "front"})
    assert [b["id"] for b in cad.result("bed.list")["beds"]] == [front, back]
    b = counts("b.glb")
    assert b["Grass"] > a["Grass"] and b["Gravel"] < a["Gravel"]


def test_beds_from_closed_shapes_keep_the_class(cad):
    make_site(cad)
    cad.result("material.add_defaults")
    cls = cad.execute({"op": "add_class", "name": "Lawn"})[0]
    poly = cad.execute({"op": "add_object", "shape": {"kind": "polyline", "points": SQUARE, "closed": True}, "class": cls})[0]
    ids = cad.result("bed.add", {"from": poly, "texture": "Grass"})["ids"]
    assert len(ids) == 1
    assert cad.result("objects.list", {"kind": "polyline"}) == []
    assert cad.result("bed.get", {"id": ids[0]})["class"] == cls
    # By class: the bed has no effect until its class has a material.
    cad.result("bed.set", {"id": ids[0], "texture": None})
    assert cad.result("bed.get", {"id": ids[0]})["active"] is False
    cad.result("material.assign", {"material": "Gravel", "class": cls})
    row = cad.result("bed.get", {"id": ids[0]})
    assert row["active"] and row["material"] == "Gravel" and row["by_class"]
    # An open or non-closed shape is refused.
    line = cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [10, 10]}})[0]
    assert "not a closed shape" in cad.error("bed.add", {"from": line})["message"]


def test_beds_work_with_the_elevation_look(cad, tmp_path):
    site, _ = make_site(cad)
    cad.result("material.add_defaults")
    cad.result("site.texture", {"id": site, "look": "elevation", "bands": 4, "ramp": [[0, 0, 255], [255, 0, 0]]})
    cad.result("bed.add", {"boundary": SQUARE, "texture": "Grass"})
    glb = tmp_path / "e.glb"
    cad.result("export.file", {"path": str(glb), "format": "glb"})
    doc = glb_json(glb)
    assert len(doc["images"]) == 1
    flat = [m for m in doc["materials"] if "baseColorTexture" not in m["pbrMetallicRoughness"]]
    assert len({json.dumps(m["pbrMetallicRoughness"]["baseColorFactor"]) for m in flat}) >= 3


def test_bad_values_are_refused_and_change_nothing(cad):
    make_site(cad)
    cad.result("material.add_defaults")
    bed = cad.result("bed.add", {"boundary": SQUARE, "texture": "Grass"})["id"]
    assert "tile" in cad.error("bed.set", {"id": bed, "tile_size": 1})["message"]
    assert "no material" in cad.error("bed.add", {"boundary": SQUARE, "texture": "Nothing"})["message"]
    assert "3 or more" in cad.error("bed.add", {"boundary": [[0, 0], [1, 1]]})["message"]
    assert len(cad.result("bed.list")["beds"]) == 1
    assert cad.result("bed.get", {"id": bed})["own_mapping"] is False


def test_ifc_export_is_unchanged_by_beds(cad, tmp_path):
    ifcopenshell = __import__("pytest").importorskip("ifcopenshell")
    make_site(cad)
    cad.result("material.add_defaults")

    def entities(name):
        path = tmp_path / name
        cad.result("export.file", {"path": str(path), "format": "ifc"})
        model = ifcopenshell.open(str(path))
        counts = {}
        for e in model:
            counts[e.is_a()] = counts.get(e.is_a(), 0) + 1
        return counts

    plain = entities("plain.ifc")
    cad.result("bed.add", {"boundary": SQUARE, "texture": "Grass"})
    with_bed = entities("bed.ifc")
    assert plain == with_bed


def test_python_api_has_the_bed_methods(cad):
    names = {m["name"] for m in cad.result("describe")["methods"]}
    assert {"bed.add", "bed.set", "bed.get", "bed.list"} <= names
    assert len(cad.result("describe")["texture_beds"]["examples"]) >= 4
