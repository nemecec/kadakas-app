"""Scenario: a DXF file from a CAD program has entities that the import used to skip (IOP-14,
IOP-15). The file is made with ezdxf. Every entity is imported or has a line in the report."""

import pytest
from pytest import approx

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
from ezdxf.render import MeshBuilder  # noqa: E402


@pytest.fixture
def entities_file(tmp_path):
    doc = ezdxf.new("R2018", setup=True)
    doc.units = 4
    msp = doc.modelspace()
    # A mesh (cube), a wipeout over a solid hatch, a solid, a leader, a linear dimension.
    cube = MeshBuilder()
    cube.add_mesh(vertices=[(0, 0, 0), (1000, 0, 0), (1000, 1000, 0), (0, 1000, 0), (0, 0, 1000), (1000, 0, 1000), (1000, 1000, 1000), (0, 1000, 1000)],
                  faces=[(0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)])
    cube.render_mesh(msp, dxfattribs={"layer": "MESHES"})
    hatch = msp.add_hatch(color=1, dxfattribs={"layer": "FILLS"})
    hatch.paths.add_polyline_path([(0, 0), (4000, 0), (4000, 4000), (0, 4000)], is_closed=True)
    hatch.paths.add_polyline_path([(1000, 1000), (3000, 1000), (3000, 3000), (1000, 3000)], is_closed=True)
    wipe = msp.add_wipeout([(500, 500), (1500, 500), (1500, 1500), (500, 1500)])
    wipe.dxf.layer = "FILLS"  # add_wipeout resets the attributes, so the layer is set after it
    assert wipe.dxftype() == "WIPEOUT"
    msp.add_solid([(0, 5000), (1000, 5000), (0, 6000), (1000, 6000)], dxfattribs={"layer": "FILLS", "color": 3})
    msp.add_leader([(0, 7000), (1000, 8000), (2000, 8000)], dxfattribs={"layer": "NOTES"})
    msp.add_linear_dim(base=(0, -1000), p1=(0, 0), p2=(3000, 0), dxfattribs={"layer": "DIMS"}).render()
    msp.add_mtext("{\\fArial|b0;Bold}\\Pline %%c20 \\S1/2;", dxfattribs={"layer": "NOTES", "insert": (0, 9000)})
    msp.add_spline(fit_points=[(0, 0), (1000, 1000), (2000, 0)], dxfattribs={"layer": "CURVES"})
    msp.add_ellipse((0, 0), major_axis=(1000, 0), ratio=0.5, dxfattribs={"layer": "CURVES"})
    # A circle in a vertical plane (normal along X): it must stand up.
    msp.add_circle((0, 0, 0), 500, dxfattribs={"layer": "TILT", "extrusion": (1, 0, 0)})
    # An entity that is reported by name.
    image_def = doc.add_image_def(filename="photos/site.png", size_in_pixel=(100, 100))
    msp.add_image(image_def, insert=(0, 0), size_in_units=(1000, 1000), rotation=0)
    path = tmp_path / "entities.dxf"
    doc.saveas(path)
    return path


def test_entities_are_imported_or_reported(cad, entities_file):
    r = cad.result("import.file", {"path": str(entities_file), "options": {"track_source": False}})
    rep = r["report"]
    assert rep["entities_read"] == rep.get("entities_imported", 0) + rep.get("entities_skipped", 0), rep
    skipped = {s["entity"]: s["reason"] for s in rep["skipped"]}
    assert "'photos/site.png'" in skipped["IMAGE"]
    kinds = rep["objects"]
    assert kinds.get("mesh3d") == 1
    # The definition points of the dimension are not survey points.
    assert "locus3d" not in kinds
    assert skipped["POINT"].startswith("definition point of a dimension")
    notes = " ".join(rep["notes"])
    assert "WIPEOUT became polygons" in notes or "WIPEOUTs became polygons" in notes
    assert "LEADER was imported as lines" in notes
    assert "island" in notes and "spline" in notes and "ellipse" in notes
    assert "tilted plane" in notes
    # The dimension block gave lines and text on the layer of the dimension.
    classes = {c["name"]: c["id"] for c in cad.result("document.summary")["classes"]}
    dims = cad.result("objects.list", {"class": classes["DIMS"]})
    assert len(dims) >= 4
    assert any(o["shape"]["kind"] == "text" for o in dims)
    # The wipeout is white, has no line and lies above the hatch it covers.
    objs = cad.result("objects.list", {"class": classes["FILLS"]})
    wipe = [o for o in objs if o["attrs"].get("fill") == [255, 255, 255, 255] and o["attrs"].get("pen") == [0, 0, 0, 0]]
    assert len(wipe) == 1, [o["attrs"] for o in objs]
    order = [o["id"] for o in objs]
    hatch_ids = [o["id"] for o in objs if o["shape"]["kind"] == "polyline" and o["attrs"].get("fill") == [255, 0, 0, 255]]
    assert hatch_ids and order.index(wipe[0]["id"]) > order.index(hatch_ids[0])
    # The circle in the vertical plane is a closed 3D polyline with points up to a height of 500.
    tilt = cad.result("objects.list", {"class": classes["TILT"]})
    assert len(tilt) == 1 and tilt[0]["shape"]["kind"] == "polyline3d"
    zs = [p[2] for p in tilt[0]["shape"]["points"]]
    assert max(zs) == approx(500, abs=1) and min(zs) == approx(-500, abs=1)
    # The MTEXT lost its codes.
    text = [o for o in cad.result("objects.list", {"class": classes["NOTES"]}) if o["shape"]["kind"] == "text"]
    assert text[0]["shape"]["text"].startswith("Bold\nline \u00d820 1/2")
