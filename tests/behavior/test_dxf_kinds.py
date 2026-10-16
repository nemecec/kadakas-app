"""Scenario: a DXF export of a whole design has irrigation, massing and plant tags, and says
that nothing was left out. Checked with ezdxf."""

import pytest

from all_kinds import build_all_kinds

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")


def export(cad, tmp_path, name="kinds.dxf", options=None):
    path = tmp_path / name
    params = {"format": "dxf", "path": str(path)}
    if options:
        params["options"] = options
    r = cad.result("export.file", params)
    doc = ezdxf.readfile(str(path))
    auditor = doc.audit()
    assert not auditor.errors, [e.message for e in auditor.errors]
    return doc, r


def test_irrigation_massing_and_plant_tags_are_exported(cad, tmp_path):
    made = build_all_kinds(cad)
    assert cad.result("objects.list", {"kind": "plant_tag"}), "plants.place with tag makes a plant tag"
    doc, r = export(cad, tmp_path)
    report = r["report"]
    assert not [s for s in report["skipped"] if "irrigation" in s["entity"] or s["entity"] in ("massing", "plant_tag")], report["skipped"]
    assert report["objects"]["plant_tag"] == 1 and report["objects"]["massing"] == 3
    assert report["objects"]["irrigation_outlet"] == 1 and report["objects"]["irrigation_controller"] == 1
    texts = [t.dxf.text for t in doc.modelspace().query("TEXT")]
    assert any(t.startswith("LA") for t in texts), texts
    layers = {layer.dxf.name for layer in doc.layers}
    assert {"IRR-OUTLET", "IRR-MAINLINE", "IRR-VALVE", "IRR-POC", "IRR-CONTROLLER", "IRR-LATERAL"} <= layers
    msp = doc.modelspace()
    valves = [e for e in msp.query("LWPOLYLINE") if e.dxf.layer == "IRR-VALVE"]
    assert len(valves) == 1
    data = dict(item[1].split("=", 1) for item in valves[0].get_xdata("KADAKAS") if item[0] == 1000)
    assert data["kind"] == "irrigation_valve" and data["name"] == "V1" and data["zone"] == "1" and data["kv"] == "6"
    # Massing: an outline with its heights, and a polyface mesh whose top is higher than its base.
    outlines = [e for e in msp.query("LWPOLYLINE") if e.has_xdata("KADAKAS") and dict(
        i[1].split("=", 1) for i in e.get_xdata("KADAKAS") if i[0] == 1000).get("kind") == "massing"]
    assert len(outlines) == 3
    gable = [o for o in outlines if "roof=gable" in [i[1] for i in o.get_xdata("KADAKAS")]]
    assert len(gable) == 1
    assert "height_mm=6000" in [i[1] for i in gable[0].get_xdata("KADAKAS")]
    meshes = [e for e in msp.query("POLYLINE") if e.is_poly_face_mesh]
    assert len(meshes) == 3
    assert max(v.dxf.location.z for m in meshes for v in m.vertices) > 6000
    assert made["massing"]


def test_massing_mesh_follows_the_mesh_entity_option(cad, tmp_path):
    cad.result("massing.add", {"footprint": [[0, 0], [8000, 0], [8000, 5000], [0, 5000]], "height": 3000, "roof": "hip"})
    doc, r = export(cad, tmp_path, "faces.dxf", {"mesh_entity": "face3d"})
    assert len(list(doc.modelspace().query("3DFACE"))) >= 10
    assert not [s for s in r["report"]["skipped"]]
