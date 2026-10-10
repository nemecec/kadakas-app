"""Scenario: native DXF entities (DIMENSION, LEADER, MTEXT), 3D mesh entities and GEODATA, checked with ezdxf."""

import pytest

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")


def export(cad, tmp_path, name="out.dxf", options=None):
    path = tmp_path / name
    params = {"format": "dxf", "path": str(path)}
    if options:
        params["options"] = options
    r = cad.result("export.file", params)
    doc = ezdxf.readfile(str(path))
    auditor = doc.audit()
    assert not auditor.errors, [e.message for e in auditor.errors]
    assert not auditor.fixes, [e.message for e in auditor.fixes]
    from ezdxf import recover

    _, rec = recover.readfile(str(path))
    assert not rec.errors and not rec.fixes
    return doc, r


def test_dimensions_callouts_and_text_are_native_entities(cad, tmp_path):
    ln = cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}})[0]
    cad.result("dimension.add", {"mode": "horizontal", "points": [[0, 0], [5000, 0]]})
    cad.result("dimension.add", {"mode": "aligned", "points": [[0, 2000], [3000, 3000]], "offset": 6})
    cad.result("dimension.add", {"mode": "radial", "points": [[10000, 0], [12000, 0]]})
    cad.result("dimension.add", {"mode": "diameter", "points": [[10000, 5000], [12000, 5000]]})
    cad.result("dimension.add", {"mode": "chain_horizontal", "points": [[0, 8000], [2000, 8000], [4000, 8000]]})
    cad.result("callout.add", {"at": [3000, 3000], "leaders": [{"object": ln, "vertex": 1}], "text": "Verify\non site"})
    cad.result("annotation.text", {"at": [0, -2000], "text": "Planting notes\nsecond line"})
    doc, r = export(cad, tmp_path)
    msp = doc.modelspace()
    dims = list(msp.query("DIMENSION"))
    assert sorted(d.dimtype for d in dims) == [32, 32, 35, 36] or sorted(d.dimtype & 7 for d in dims) == [0, 0, 3, 4]
    for d in dims:
        # Each dimension has its block, and the block has the drawing.
        block = doc.blocks.get(d.dxf.geometry)
        assert len(block) >= 3
        assert block.block_record.dxf.name.startswith("*D")
    linear = [d for d in dims if d.dimtype & 7 == 0]
    assert sorted(d.dxf.text for d in linear) == ["3162", "5000"]
    assert {tuple(round(v) for v in d.dxf.defpoint2) for d in linear} >= {(0, 0, 0)}
    leaders = list(msp.query("LEADER"))
    assert len(leaders) == 1 and len(list(leaders[0].vertices)) == 2
    texts = [m.text for m in msp.query("MTEXT")]
    assert "Planting notes\\Psecond line" in texts and "Verify\\Pon site" in texts
    # The chain dimension has no native form: it is exploded into lines and text.
    assert "2000" in [t.dxf.text for t in msp.query("TEXT")]
    assert r["report"]["skipped"] == []


def test_native_annotations_can_be_switched_off(cad, tmp_path):
    cad.result("dimension.add", {"mode": "horizontal", "points": [[0, 0], [5000, 0]]})
    cad.result("annotation.text", {"at": [0, -2000], "text": "Notes"})
    doc, _ = export(cad, tmp_path, options={"native_annotations": False})
    msp = doc.modelspace()
    assert not list(msp.query("DIMENSION")) and not list(msp.query("MTEXT"))
    assert "5000" in [t.dxf.text for t in msp.query("TEXT")]


def test_dimension_blocks_follow_the_unit_option(cad, tmp_path):
    cad.result("dimension.add", {"mode": "horizontal", "points": [[0, 0], [5000, 0]]})
    doc, _ = export(cad, tmp_path, options={"unit": "m"})
    (d,) = doc.modelspace().query("DIMENSION")
    assert tuple(round(v, 3) for v in d.dxf.defpoint3) == (5.0, 0.0, 0.0)
    assert doc.header["$INSUNITS"] == 6


def test_a_mesh_is_a_polyface_mesh_or_3d_faces(cad, tmp_path):
    cad.execute({"op": "add_object", "shape": {"kind": "mesh3d", "vertices": [[0, 0, 0], [1000, 0, 0], [1000, 1000, 500], [0, 1000, 500]], "triangles": [[0, 1, 2], [0, 2, 3]]}})
    doc, _ = export(cad, tmp_path)
    (pm,) = doc.modelspace().query("POLYLINE")
    assert pm.is_poly_face_mesh and pm.dxf.m_count == 4 and pm.dxf.n_count == 2
    faces = list(pm.faces()) if hasattr(pm, "faces") else []
    assert len(faces) == 2
    doc, _ = export(cad, tmp_path, "faces.dxf", {"mesh_entity": "face3d"})
    faces = list(doc.modelspace().query("3DFACE"))
    assert len(faces) == 2
    assert sorted(round(v) for f in faces for v in (f.dxf.vtx0[2], f.dxf.vtx2[2])) == [0, 0, 500, 500]
    # The polyface mesh imports again as one mesh with the same triangles.
    cad.result("undo")
    cad.result("import.file", {"path": str(tmp_path / "out.dxf"), "options": {"origin": "file"}})
    (mesh,) = cad.result("objects.list", {"kind": "mesh3d"})
    assert len(mesh["shape"]["vertices"]) == 4 and len(mesh["shape"]["triangles"]) == 2


def test_the_site_surface_can_be_written_as_a_mesh(cad, tmp_path):
    ids = []
    for i in range(4):
        for j in range(4):
            ids.append(cad.execute({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}})[0])
    cad.result("site.create", {"source_ids": ids})
    doc, _ = export(cad, tmp_path, "plain.dxf")
    assert not list(doc.modelspace().query("POLYLINE")) or all(not p.is_poly_face_mesh for p in doc.modelspace().query("POLYLINE"))
    doc, _ = export(cad, tmp_path, "mesh.dxf", {"site_mesh": "existing"})
    meshes = [p for p in doc.modelspace().query("POLYLINE") if p.is_poly_face_mesh]
    assert len(meshes) == 1 and meshes[0].dxf.m_count == 16 and meshes[0].dxf.n_count == 18


def test_the_georeference_is_a_geodata_object(cad, tmp_path):
    cad.result("georef.survey_point", {"model": [0, 0], "map": [385000, 6672000], "crs": "EPSG:3067"})
    cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}})
    doc, r = export(cad, tmp_path, options={"unit": "m"})
    geo = list(doc.objects.query("GEODATA"))
    assert len(geo) == 1
    g = geo[0]
    assert g.dxf.coordinate_type == 2
    assert tuple(g.dxf.reference_point) == (385000.0, 6672000.0, 0.0)
    # Real-world coordinates are written, so the design point is the same place.
    assert tuple(g.dxf.design_point) == (385000.0, 6672000.0, 0.0)
    assert "EPSG:3067" in g.coordinate_system_definition
    assert g.dxf.horizontal_units == 6 and g.dxf.horizontal_unit_scale == 1.0
    (line,) = doc.modelspace().query("LINE")
    assert tuple(round(v, 3) for v in line.dxf.start) == (385000.0, 6672000.0, 0.0)
    # In document coordinates the design point is the origin of the drawing.
    doc, _ = export(cad, tmp_path, "doc.dxf", {"origin": "document", "unit": "m"})
    (g,) = doc.objects.query("GEODATA")
    assert tuple(g.dxf.design_point) == (0.0, 0.0, 0.0)
    assert tuple(g.dxf.reference_point) == (385000.0, 6672000.0, 0.0)


def test_a_crs_without_an_epsg_code_gets_a_note_and_no_geodata(cad, tmp_path):
    cad.execute({"op": "set_georeference", "georef": {"crs": "PROJCS[\"x\"]", "origin": [100, 200]}})
    cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}})
    doc, r = export(cad, tmp_path)
    assert not list(doc.objects.query("GEODATA"))
    assert any("not an EPSG code" in n for n in r["report"]["notes"])


def test_import_of_3d_entities_from_ezdxf(cad, tmp_path):
    doc = ezdxf.new("R2004")
    msp = doc.modelspace()
    msp.add_3dface([(0, 0, 0), (1, 0, 0), (1, 1, 1), (0, 1, 1)], dxfattribs={"layer": "Faces"})
    msp.add_3dface([(1, 0, 0), (2, 0, 0), (2, 1, 2)], dxfattribs={"layer": "Faces"})
    pf = msp.add_polyface(dxfattribs={"layer": "Poly"})
    pf.append_faces([[(0, 0, 5), (1, 0, 5), (1, 1, 5), (0, 1, 5)], [(1, 0, 5), (2, 0, 5), (1, 1, 5)]])
    pf.optimize()
    pm = msp.add_polymesh(size=(3, 3), dxfattribs={"layer": "Grid"})
    for i in range(3):
        for j in range(3):
            pm.set_mesh_vertex((i, j), (i, j, i + j))
    msp.add_polyline3d([(0, 0, 0), (1, 2, 3), (4, 5, 6)], dxfattribs={"layer": "Line3d"})
    path = tmp_path / "three_d.dxf"
    doc.saveas(str(path))
    unit_m = {"unit": "m", "origin": "file"}
    r = cad.result("import.file", {"path": str(path), "options": unit_m})
    assert r["skipped"] == 0, r["report"]
    kinds = {}
    for o in cad.result("objects.list"):
        kinds.setdefault(o["shape"]["kind"], []).append(o)
    assert len(kinds["polyline3d"]) == 3  # two 3DFACE polygons and the 3D polyline
    meshes = sorted(kinds["mesh3d"], key=lambda o: len(o["shape"]["triangles"]))
    assert [len(m["shape"]["triangles"]) for m in meshes] == [3, 8]
    # The polyface is a quad and a triangle: 3 triangles. Heights are in mm.
    assert {v[2] for v in meshes[0]["shape"]["vertices"]} == {5000.0}
    # The polygon mesh of 3 by 3 vertices has 8 triangles.
    assert len(meshes[1]["shape"]["vertices"]) == 9
    # Faces can be merged into one mesh per layer.
    cad.result("undo")
    cad.result("import.file", {"path": str(path), "options": {**unit_m, "faces": "mesh"}})
    meshes = cad.result("objects.list", {"kind": "mesh3d"})
    assert len(meshes) == 3
    assert len(cad.result("objects.list", {"kind": "polyline3d"})) == 1
    merged = [m for m in meshes if len(m["shape"]["triangles"]) == 3 and len(m["shape"]["vertices"]) == 6]
    assert merged, [len(m["shape"]["vertices"]) for m in meshes]
