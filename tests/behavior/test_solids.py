"""Scenario: a landscape architect models a planter (extrude), a pot and a spiral ramp (sweep) and a sloped
deck face (3D polygon), changes them, undoes a change, and exports the model to glTF, IFC and DXF.
The files are read back with the standard library and ezdxf."""

import json
import struct

import pytest
from pytest import approx


def rect(x, y, w, d):
    return [[x, y], [x + w, y], [x + w, y + d], [x, y + d]]


def glb_json(data):
    assert data[:4] == b"glTF"
    length = struct.unpack("<I", data[12:16])[0]
    return json.loads(data[20:20 + length])


def test_extrude_keeps_its_profile_and_height_editable(cad):
    planter = cad.result("extrude.add", {
        "profile": rect(0, 0, 2000, 1000), "holes": [rect(500, 250, 400, 400)], "height": 450, "name": "Planter"})
    assert planter["quantities"]["volume_m3"] == approx((2.0 - 0.16) * 0.45)
    got = cad.result("extrude.get", {"id": planter["id"]})
    assert got["profile"][2] == [2000, 1000] and len(got["holes"]) == 1
    assert got["bounds_mm"]["max"] == [2000, 1000, 450]
    cad.result("extrude.set", {"id": planter["id"], "height": 900, "profile": rect(0, 0, 3000, 1000), "holes": None})
    got = cad.result("extrude.get", {"id": planter["id"]})
    assert got["height"] == 900 and got["holes"] == []
    assert got["quantities"]["volume_m3"] == approx(3.0 * 0.9)
    assert cad.result("undo")["done"]
    assert cad.result("extrude.get", {"id": planter["id"]})["height"] == 450
    # The profile vertices are editable like a polyline's.
    cad.execute({"op": "set_vertex", "id": planter["id"], "index": 2, "p": [2500, 1500]})
    assert cad.result("extrude.get", {"id": planter["id"]})["profile"][2] == [2500, 1500]


def test_bad_solids_are_refused_with_a_reason(cad):
    sq = rect(0, 0, 100, 100)
    assert "zero" in cad.error("extrude.add", {"profile": sq, "height": 0})["message"]
    bow = [[0, 0], [10, 10], [10, 0], [0, 10]]
    assert "itself" in cad.error("extrude.add", {"profile": bow, "height": 5})["message"]
    assert "hole" in cad.error("extrude.add", {"profile": sq, "height": 5, "holes": [rect(80, 80, 50, 50)]})["message"]
    assert "axis" in cad.error("sweep.add", {"profile": rect(-50, 0, 100, 100)})["message"]
    assert "sweep angle" in cad.error("sweep.add", {"profile": rect(10, 0, 100, 100), "sweep_angle": 0})["message"]
    assert "unknown field" in cad.error("sweep.add", {"profile": rect(10, 0, 100, 100), "speed": 3})["message"]
    assert cad.doc()["objects"] == [] or len(cad.doc()["objects"]) == 0


def test_sweep_segments_pitch_and_the_cap(cad):
    pot = cad.result("sweep.add", {"profile": [[0, 0], [400, 0], [300, 800], [0, 800]], "origin": [5000, 5000, 0], "name": "Pot"})
    got = cad.result("sweep.get", {"id": pot["id"]})
    assert got["segments"] == 36 and got["bounds_mm"]["max"][2] == approx(800)
    assert got["quantities"]["volume_m3"] > 0.1
    ramp = cad.result("sweep.add", {"profile": rect(800, 0, 400, 150), "sweep_angle": 720, "pitch": 1500, "segment_angle": 8})
    assert cad.result("sweep.get", {"id": ramp["id"]})["bounds_mm"]["max"][2] == approx(3150)
    capped = cad.result("sweep.set", {"id": pot["id"], "sweep_angle": 3600, "segment_angle": 0.1})
    assert len(capped["warnings"]) == 1
    assert cad.result("sweep.get", {"id": pot["id"]})["segments"] == 720
    surface = cad.result("sweep.add", {"profile": [[100, 0], [500, 800]], "closed": False})
    assert surface["quantities"]["volume_m3"] is None and surface["quantities"]["surface_area_m2"] > 0


def test_model_extrude_and_sweep_convert_drawn_shapes(cad):
    shapes = cad.execute({"op": "add_object", "shape": {"kind": "polyline", "closed": True, "points": rect(0, 0, 1000, 1000)}})
    label = cad.execute({"op": "add_object", "shape": {"kind": "text", "at": [0, 0], "text": "t", "height": 10}})
    made = cad.result("extrude.from_shapes", {"ids": [shapes[0], label[0]], "height": 700})
    assert len(made["created"]) == 1 and made["skipped"][0]["reason"] == "a text is not a closed shape"
    assert cad.result("extrude.get", {"id": made["created"][0]["id"]})["height"] == 700
    # The 2D shape became the solid, and undo brings it back.
    kinds = [o["shape"]["kind"] for o in cad.doc()["objects"]]
    assert "extrude" in kinds and kinds.count("polyline") == 0
    assert cad.result("undo")["done"]
    assert "polyline" in [o["shape"]["kind"] for o in cad.doc()["objects"]]
    axis = cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 3000], "b": [4000, 3000]}})
    ring = cad.execute({"op": "add_object", "shape": {"kind": "rect", "min": [1000, 3500], "max": [1600, 4100]}})
    swept = cad.result("sweep.from_shapes", {"ids": [ring[0]], "axis_id": axis[0], "sweep_angle": 180})
    assert len(swept["created"]) == 1
    got = cad.result("sweep.get", {"id": swept["created"][0]["id"]})
    assert got["axis"] == [1.0, 0.0, 0.0] and got["origin"] == [0.0, 3000.0, 0.0]


def test_3d_polygon_is_a_face_when_it_has_a_fill(cad):
    deck = cad.result("polygon3d.add", {
        "points": [[0, 0, 0], [4000, 0, 300], [4000, 3000, 600], [0, 3000, 300]], "fill": [160, 184, 144, 255]})
    got = cad.result("polygon3d.get", {"id": deck["id"]})
    assert got["z_range_mm"] == [0, 600] and got["face"] and got["area_plan_m2"] == approx(12.0)
    cad.result("polygon3d.set", {"id": deck["id"], "raise": 1000})
    assert cad.result("polygon3d.get", {"id": deck["id"]})["z_range_mm"] == [1000, 1600]
    assert "closed" in cad.error("polygon3d.add", {"points": [[0, 0, 0], [1, 1, 1]]})["message"]


def test_the_model_exports_to_gltf_ifc_and_dxf(cad, tmp_path):
    cad.result("extrude.add", {"profile": rect(0, 0, 2000, 1000), "holes": [rect(500, 250, 400, 400)], "height": 450, "name": "Planter"})
    cad.result("sweep.add", {"profile": [[0, 0], [400, 0], [300, 800], [0, 800]], "origin": [5000, 0, 0], "name": "Pot"})
    glb = tmp_path / "m.glb"
    assert cad.result("export.file", {"path": str(glb), "format": "glb"})["exported"] == 2
    j = glb_json(glb.read_bytes())
    assert sorted(n["extras"]["kind"] for n in j["nodes"]) == ["extrude", "sweep"]
    ifc = tmp_path / "m.ifc"
    cad.result("export.file", {"path": str(ifc), "format": "ifc"})
    text = ifc.read_text()
    assert "IFCEXTRUDEDAREASOLID" in text and "IFCARBITRARYPROFILEDEFWITHVOIDS" in text and "IFCTRIANGULATEDFACESET" in text
    dxf = tmp_path / "m.dxf"
    cad.result("export.file", {"path": str(dxf), "format": "dxf"})
    ezdxf = pytest.importorskip("ezdxf")
    doc = ezdxf.readfile(str(dxf))
    auditor = doc.audit()
    assert not auditor.has_errors
    meshes = [e for e in doc.modelspace() if e.dxftype() == "POLYLINE" and e.is_poly_face_mesh]
    assert len(meshes) == 2
    # The section through the planter cuts a face.
    line = cad.result("section_viewport.line_add", {"a": [-500, 500], "b": [2500, 500], "name": "A"})["id"]
    section = cad.result("section_viewport.get", {"id": line, "geometry": True})
    assert section is not None
