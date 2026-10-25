"""Scenario: a designer moves 3D models between Kadakas and other programs (MOD-12).

OBJ, STL and glTF files come in as meshes and go out from surface models. The IFC export of surface
models is read back by IfcOpenShell, the DXF export by ezdxf and the glTF export by the Khronos
validator. Each check skips when its tool is missing. Broken files must be refused cleanly."""

import json
import shutil
import struct
import subprocess
from pathlib import Path

import pytest
from pytest import approx

from helpers import isolated_env

IFC_CHECK = Path(__file__).with_name("ifc_check.py")
IFC_VOLUME = Path(__file__).with_name("ifc_volume_check.py")

CUBE_OBJ = (
    "o Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n"
    "f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n"
)


def uv_run(script, path, *packages):
    """Runs a checker script with `uv` and the packages. Skips when uv or a package is missing."""
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    args = ["uv", "run", "--no-project"]
    for p in packages:
        args += ["--with", p]
    run = subprocess.run(
        [*args, "python", "-I", str(script), str(path)],
        capture_output=True, text=True, encoding="utf-8", timeout=600, env=isolated_env(uv=True),
    )
    if run.returncode != 0 and "No solution found" in run.stderr + run.stdout:
        pytest.skip(f"{packages} is not available")
    assert run.returncode == 0, run.stderr
    return json.loads(run.stdout.splitlines()[-1])


def slab_with_hole(cad):
    """A slab 4 by 3 m and 0.5 m thick with a hole of 1 by 1 m: a closed solid with ten faces."""
    model = cad.result("mesh.create", {"name": "Slab"})["id"]

    def rect(x0, y0, x1, y1):
        return [[[x0, y0, 0], [x1, y0, 0]], [[x1, y0, 0], [x1, y1, 0]], [[x1, y1, 0], [x0, y1, 0]], [[x0, y1, 0], [x0, y0, 0]]]

    cad.result("mesh.add_edges", {"id": model, "edges": rect(0, 0, 4000, 3000) + rect(1000, 1000, 2000, 2000)})
    faces = cad.result("mesh.faces", {"id": model})["items"]
    faces.sort(key=lambda f: f["area_mm2"])
    cad.result("mesh.erase", {"id": model, "faces": [faces[0]["id"]]})
    cad.result("mesh.push_pull", {"id": model, "face": faces[1]["id"], "distance": 500})
    report = cad.result("mesh.solid_report", {"id": model})
    assert report["closed"]
    return model


def open_plate(cad):
    model = cad.result("mesh.create", {"name": "Plate", "origin": [10000, 0, 0]})["id"]
    cad.result("mesh.add_edges", {"id": model, "edges": [
        [[0, 0, 0], [1000, 0, 0]], [[1000, 0, 0], [1000, 1000, 0]],
        [[1000, 1000, 0], [0, 1000, 0]], [[0, 1000, 0], [0, 0, 0]]]})
    return model


# ---------------------------------------------------------------- IFC


@pytest.mark.parametrize("version", ["IFC4", "IFC2X3", "IFC4X3"])
def test_surface_models_export_as_valid_ifc_faceted_solids(cad, tmp_path, version):
    slab = slab_with_hole(cad)
    out = tmp_path / "slab.ifc"
    r = cad.result("export.file", {"path": str(out), "format": "ifc", "options": {"version": version}})
    assert r["exported"] == 1, r
    facts = uv_run(IFC_CHECK, out, "ifcopenshell")
    assert facts["validation"] == [], facts["validation"]
    assert facts["counts"]["IfcFacetedBrep"] == 1
    assert facts["counts"]["IfcClosedShell"] == 1
    assert facts["counts"]["IfcFace"] == 10
    assert facts["counts"]["IfcFaceBound"] == 2, "the hole of the top and of the bottom"
    assert facts["counts"].get("IfcShellBasedSurfaceModel", 0) == 0
    volume = uv_run(IFC_VOLUME, out, "ifcopenshell")
    assert len(volume) == 1
    assert volume[0]["name"] == "Slab"
    assert volume[0]["items"] == ["IfcFacetedBrep"]
    # (4 x 3 - 1 x 1) m2 x 0.5 m
    assert volume[0]["volume"] == approx(5.5, rel=1e-4)
    assert volume[0]["triangles"] >= 20


def test_open_surface_models_export_as_shells_with_a_note(cad, tmp_path):
    slab_with_hole(cad)
    open_plate(cad)
    out = tmp_path / "mixed.ifc"
    r = cad.result("export.file", {"path": str(out), "format": "ifc"})
    assert r["exported"] == 2
    notes = " ".join(r["report"].get("notes", []))
    assert "not in a closed solid" in notes, notes
    facts = uv_run(IFC_CHECK, out, "ifcopenshell")
    assert facts["validation"] == [], facts["validation"]
    assert facts["counts"]["IfcFacetedBrep"] == 1
    assert facts["counts"]["IfcShellBasedSurfaceModel"] == 1
    assert facts["counts"]["IfcOpenShell"] == 1
    volume = uv_run(IFC_VOLUME, out, "ifcopenshell")
    by_name = {v["name"]: v for v in volume}
    assert by_name["Plate"]["area"] == approx(1.0, rel=1e-4)


# ---------------------------------------------------------------- DXF


def test_dxf_export_of_surface_models_passes_the_ezdxf_audit(cad, tmp_path):
    ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")
    slab_with_hole(cad)
    counts = {}
    for entity, kind in [("polyface", "POLYLINE"), ("face3d", "3DFACE"), ("mesh", "MESH")]:
        out = tmp_path / f"{entity}.dxf"
        cad.result("export.file", {"path": str(out), "format": "dxf", "options": {"mesh_entity": entity}})
        doc = ezdxf.readfile(str(out))
        auditor = doc.audit()
        assert not auditor.errors, [e.message for e in auditor.errors]
        msp = doc.modelspace()
        found = list(msp.query(kind))
        assert found, entity
        counts[entity] = len(found)
        if entity == "mesh":
            mesh = found[0]
            assert len(mesh.vertices) == 16
            assert len(list(mesh.faces)) >= 20
            zs = sorted({round(v[2]) for v in mesh.vertices})
            assert zs == [0, 500]
        assert doc.dxfversion == "AC1018", "the MESH entity is audited in the 2004 file"
        if entity == "face3d":
            assert len(found) >= 20
            # The diagonals that cut a face into triangles are not drawn.
            assert any(f.dxf.invisible_edges for f in found)
            assert {round(p[2]) for f in found for p in f.wcs_vertices()} == {0, 500}
    assert counts["polyface"] == 1 and counts["mesh"] == 1


# ---------------------------------------------------------------- import and export of meshes


def write_stl(path, triangles):
    data = bytearray(b"a test file".ljust(80, b" "))
    data += struct.pack("<I", len(triangles))
    for t in triangles:
        data += struct.pack("<3f", 0, 0, 0)
        for p in t:
            data += struct.pack("<3f", *p)
        data += b"\0\0"
    path.write_bytes(bytes(data))


def test_obj_stl_and_glb_import_with_units_axes_and_a_soften_report(cad, tmp_path):
    obj = tmp_path / "cube.obj"
    obj.write_text(CUBE_OBJ, encoding="utf-8")
    r = cad.result("import.file", {"path": str(obj)})
    assert r["imported"] == 1 and r["softened_edges"] == 0
    assert r["report"]["bounds_mm"]["size"] == [1000.0, 1000.0, 1000.0]
    assert r["report"]["objects"][0]["kind"] == "mesh3d"
    r = cad.result("import.file", {"path": str(obj), "options": {"unit": "mm", "up_axis": "z", "to_surface": True, "soften_below_angle": 100}})
    assert r["softened_edges"] == 12
    assert r["report"]["objects"][0]["kind"] == "surface_model"
    assert r["report"]["to_surface"]["converted"] == 1
    sid = r["report"]["objects"][0]["id"]
    summary = cad.result("mesh.summary", {"id": sid})["models"][0]
    assert (summary["vertices"], summary["edges"], summary["faces"], summary["soft_edges"]) == (8, 12, 6, 12)
    # An STL file in millimeters.
    stl = tmp_path / "tri.stl"
    write_stl(stl, [[(0, 0, 0), (100, 0, 0), (0, 100, 0)], [(100, 0, 0), (100, 100, 0), (0, 100, 0)]])
    r = cad.result("import.file", {"path": str(stl), "options": {"to_surface": True}})
    assert r["report"]["to_surface"]["faces"] == 1, "two coplanar triangles make one face"
    # The weld counts.
    assert r["report"]["vertices"] == 4


def test_a_surface_model_goes_out_and_comes_back_through_each_format(cad, tmp_path):
    slab = slab_with_hole(cad)
    before = cad.result("mesh.solid_report", {"id": slab})["shells"][0]
    for name, fmt in [("s.obj", "obj"), ("s.stl", "stl"), ("s.glb", "glb"), ("s.gltf", "gltf")]:
        path = tmp_path / name
        out = cad.result("export.file", {"path": str(path), "format": fmt, "options": {"objects": [slab]}})
        assert out["exported"] == 1
        imp = cad.result("import.file", {"path": str(path), "options": {"to_surface": True}})
        sid = imp["report"]["objects"][0]["id"]
        shell = cad.result("mesh.solid_report", {"id": sid})["shells"][0]
        assert shell["state"] == "solid", fmt
        assert shell["volume"] == approx(before["volume"], rel=1e-6), fmt
        assert shell["area"] == approx(before["area"], rel=1e-6), fmt
        assert cad.result("mesh.summary", {"id": sid})["models"][0]["faces"] == 10, fmt


def test_stl_export_reports_open_models_and_writes_binary_or_ascii(cad, tmp_path):
    slab_with_hole(cad)
    open_plate(cad)
    binary = tmp_path / "b.stl"
    r = cad.result("export.file", {"path": str(binary)})
    assert r["report"]["open_models"] == 1
    data = binary.read_bytes()
    n = struct.unpack("<I", data[80:84])[0]
    assert len(data) == 84 + 50 * n == 84 + 50 * r["report"]["triangles"]
    ascii_ = tmp_path / "a.stl"
    cad.result("export.file", {"path": str(ascii_), "options": {"encoding": "ascii"}})
    assert ascii_.read_text(encoding="utf-8").startswith("solid a")
    again = cad.result("export.file", {"path": str(ascii_), "options": {"encoding": "ascii"}, "overwrite": True})
    assert again["report"]["encoding"] == "ascii"


def test_the_khronos_validator_accepts_a_surface_model_export(cad, tmp_path):
    if shutil.which("node") is None or shutil.which("npm") is None:
        pytest.skip("node and npm are not installed")
    tool = tmp_path / "validator"
    tool.mkdir()
    shutil.copy(Path(__file__).with_name("gltf_validate.js"), tool / "validate.js")
    (tool / "package.json").write_text("{}", encoding="utf-8")
    install = subprocess.run([shutil.which("npm"), "install", "gltf-validator"], cwd=tool, capture_output=True, text=True, encoding="utf-8", timeout=300)
    if install.returncode != 0:
        pytest.skip("gltf-validator could not be installed")
    slab_with_hole(cad)
    obj = tmp_path / "cube.obj"
    obj.write_text(CUBE_OBJ, encoding="utf-8")
    cad.result("import.file", {"path": str(obj), "options": {"to_surface": True}})
    files = []
    for name in ("a.glb", "b.gltf"):
        cad.result("export.file", {"path": str(tmp_path / name)})
        files.append(str(tmp_path / name))
    run = subprocess.run([shutil.which("node"), "validate.js", *files], cwd=tool, capture_output=True, text=True, encoding="utf-8", timeout=300)
    assert run.returncode == 0, run.stderr
    for line in run.stdout.splitlines():
        r = json.loads(line)
        assert r["errors"] == 0 and r["warnings"] == 0, r


def test_a_gltf_file_from_another_program_imports(cad, tmp_path):
    """A glTF file that Kadakas did not write: node transforms, a material and a mesh used twice."""
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    script = tmp_path / "make_glb.py"
    script.write_text(
        "import sys, pygltflib, numpy as np\n"
        "pts = np.array([[0,0,0],[1,0,0],[0,1,0],[1,1,0]], dtype=np.float32)\n"
        "idx = np.array([0,1,2,1,3,2], dtype=np.uint16)\n"
        "g = pygltflib.GLTF2(scene=0, scenes=[pygltflib.Scene(nodes=[0,1])],\n"
        "  nodes=[pygltflib.Node(mesh=0, name='left'), pygltflib.Node(mesh=0, name='right', translation=[2,0,0])],\n"
        "  meshes=[pygltflib.Mesh(primitives=[pygltflib.Primitive(attributes=pygltflib.Attributes(POSITION=0), indices=1, material=0)])],\n"
        "  materials=[pygltflib.Material(name='red', pbrMetallicRoughness=pygltflib.PbrMetallicRoughness(baseColorFactor=[1,0,0,1]))],\n"
        "  accessors=[pygltflib.Accessor(bufferView=0, componentType=pygltflib.FLOAT, count=4, type='VEC3', max=pts.max(0).tolist(), min=pts.min(0).tolist()),\n"
        "             pygltflib.Accessor(bufferView=1, componentType=pygltflib.UNSIGNED_SHORT, count=6, type='SCALAR')],\n"
        "  bufferViews=[pygltflib.BufferView(buffer=0, byteLength=pts.nbytes), pygltflib.BufferView(buffer=0, byteOffset=pts.nbytes, byteLength=idx.nbytes)],\n"
        "  buffers=[pygltflib.Buffer(byteLength=pts.nbytes + idx.nbytes)])\n"
        "g.set_binary_blob(pts.tobytes() + idx.tobytes())\n"
        "g.save_binary(sys.argv[1])\n"
    , encoding="utf-8")
    out = tmp_path / "two.glb"
    run = subprocess.run(
        ["uv", "run", "--no-project", "--with", "pygltflib", "--with", "numpy", "python", "-I", str(script), str(out)],
        capture_output=True, text=True, encoding="utf-8", timeout=600, env=isolated_env(uv=True),
    )
    if run.returncode != 0:
        pytest.skip("pygltflib is not available")
    r = cad.result("import.file", {"path": str(out)})
    assert r["imported"] == 2
    assert [o["name"] for o in r["report"]["objects"]] == ["left", "right"]
    assert r["report"]["objects"][0]["color"] == [255, 0, 0, 255]
    assert r["report"]["triangles"] == 4
    assert r["report"]["bounds_mm"]["max"][0] == 3000.0
    # Y up: the quad stands in the XZ plane of the model.
    assert r["report"]["bounds_mm"]["size"][2] == 1000.0


def test_broken_and_hostile_files_are_refused_with_counts_and_change_nothing(cad, tmp_path):
    before = len(cad.result("objects.list", {}))
    cases = []
    stl = tmp_path / "ok.stl"
    write_stl(stl, [[(0, 0, 0), (1, 0, 0), (0, 1, 0)]])
    good = stl.read_bytes()
    cut = tmp_path / "cut.stl"
    cut.write_bytes(good[:-9])
    cases.append((cut, "mesh_truncated"))
    huge = tmp_path / "huge.stl"
    huge.write_bytes(good[:80] + struct.pack("<I", 0xFFFFFFFF) + good[84:])
    cases.append((huge, "mesh_too_many_triangles"))
    nan = tmp_path / "nan.stl"
    nan.write_bytes(good[:96] + struct.pack("<f", float("nan")) + good[100:])
    cases.append((nan, "mesh_not_finite"))
    bad = tmp_path / "bad.obj"
    bad.write_text("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 9\n", encoding="utf-8")
    cases.append((bad, "mesh_bad_index"))
    nanobj = tmp_path / "nan.obj"
    nanobj.write_text("v 0 0 nan\n", encoding="utf-8")
    cases.append((nanobj, "mesh_not_finite"))
    junk = tmp_path / "junk.glb"
    junk.write_bytes(b"glTF\x02\x00\x00\x00" + b"\xff" * 40)
    cases.append((junk, "mesh_truncated"))
    notjson = tmp_path / "text.gltf"
    notjson.write_text("this is not json", encoding="utf-8")
    cases.append((notjson, "mesh_malformed"))
    for path, reason in cases:
        e = cad.error("import.file", {"path": str(path)})
        assert e["data"]["reason"] == reason, (path.name, e)
        assert e["data"]["error_id"] == reason
    huge_limit = cad.error("import.file", {"path": str(tmp_path / "ok.stl"), "options": {"max_bytes": 20}})
    assert huge_limit["data"]["reason"] == "mesh_file_too_large"
    assert huge_limit["data"]["kind"] == "limit"
    assert huge_limit["data"]["limit"] == 20
    assert len(cad.result("objects.list", {})) == before


def test_a_mesh_over_the_surface_limits_is_refused_with_counts(cad, tmp_path):
    n = 240
    lines = []
    for j in range(n + 1):
        for i in range(n + 1):
            lines.append(f"v {i * 100} {j * 100} {((i * 73856093) ^ (j * 19349663)) % 1000}")
    for j in range(n):
        for i in range(n):
            a = j * (n + 1) + i + 1
            b, c, d = a + 1, a + n + 2, a + n + 1
            lines.append(f"f {a} {b} {c}")
            lines.append(f"f {a} {c} {d}")
    p = tmp_path / "terrain.obj"
    p.write_text("\n".join(lines) + "\n", encoding="utf-8")
    r = cad.result("import.file", {"path": str(p), "options": {"unit": "mm", "up_axis": "z", "soften_below_angle": 20}})
    assert r["softened_edges"] == 0
    assert r["report"]["objects"][0]["kind"] == "mesh3d"
    refused = r["report"]["to_surface"]["refused"][0]
    assert refused["data"]["counts"]["faces"] == 2 * n * n
    mesh = r["report"]["objects"][0]["id"]
    e = cad.error("mesh.to_surface", {"id": mesh})
    assert e["data"]["error_id"] == "mesh_too_large_for_surface"
    assert e["data"]["counts"]["faces"] == 2 * n * n
    assert {o["what"] for o in e["data"]["over"]} >= {"faces", "vertices"}


def test_describe_lists_the_mesh_formats_and_the_soften_option(cad):
    d = cad.result("describe")
    text = json.dumps(d["io"])
    for needle in ["obj, stl, gltf, glb", "soften_below_angle", "to_surface", "mesh_entity", "encoding"]:
        assert needle in text, needle
    assert "mesh.to_surface" in json.dumps(d["mesh"])
    assert "mesh_too_large_for_surface" in d["mesh"]["errors"]


def u_prism(cad):
    """A U shaped prism: the outline has corners on a line between two others."""
    model = cad.result("mesh.create", {"name": "U"})["id"]
    pts = [[0, 0], [1000, 0], [2000, 0], [3000, 0], [3000, 3000], [2000, 3000], [2000, 1000], [1000, 1000], [1000, 3000], [0, 3000]]
    edges = [[[*pts[i], 0], [*pts[(i + 1) % len(pts)], 0]] for i in range(len(pts))]
    face = cad.result("mesh.add_edges", {"id": model, "edges": edges})["created"]["faces"][0]
    cad.result("mesh.push_pull", {"id": model, "face": face, "distance": 500})
    return model


def test_a_closed_solid_gives_a_watertight_stl(cad, tmp_path):
    model = u_prism(cad)
    assert cad.result("mesh.solid_report", {"id": model})["closed"]
    out = tmp_path / "u.stl"
    r = cad.result("export.file", {"path": str(out), "options": {"objects": [model]}})
    assert r["report"]["triangles"] == 36 and r["report"]["leaky_models"] == 0
    data = out.read_bytes()
    n = struct.unpack("<I", data[80:84])[0]
    uses = {}
    for i in range(n):
        rec = data[84 + 50 * i: 84 + 50 * (i + 1)]
        pts = [struct.unpack("<3f", rec[12 + 12 * k: 24 + 12 * k]) for k in range(3)]
        for k in range(3):
            a, b = pts[k], pts[(k + 1) % 3]
            key = (min(a, b), max(a, b))
            uses[key] = uses.get(key, 0) + 1
    assert set(uses.values()) == {2}, "every edge is shared by two triangles"
    back = cad.result("import.file", {"path": str(out), "options": {"to_surface": True}})
    shell = cad.result("mesh.solid_report", {"id": back["report"]["objects"][0]["id"]})
    assert shell["closed"] is True


def test_the_model_returns_to_its_place_after_a_round_trip(cad, tmp_path):
    model = cad.result("mesh.create", {"name": "Far", "origin": [25000, 41000, 3000]})["id"]
    cad.result("mesh.add_edges", {"id": model, "edges": [
        [[0, 0, 0], [1000, 0, 0]], [[1000, 0, 0], [1000, 1000, 0]], [[1000, 1000, 0], [0, 1000, 0]], [[0, 1000, 0], [0, 0, 0]]]})
    want = cad.result("mesh.bounds", {"id": model})["world"]
    for name in ("far.obj", "far.glb", "far.stl"):
        out = tmp_path / name
        cad.result("export.file", {"path": str(out), "options": {"objects": [model]}})
        back = cad.result("import.file", {"path": str(out)})
        assert back["report"]["bounds_mm"]["min"] == approx(want["min"], abs=0.01), name
        assert back["report"]["bounds_mm"]["max"] == approx(want["max"], abs=0.01), name
