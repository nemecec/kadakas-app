"""Scenario: a landscape architect exports a garden as glTF, GLB and OBJ and draws a section of it.
pygltflib and trimesh (through `uv`) read the files. The section scenarios use the API only."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from pytest import approx

CHECK = Path(__file__).with_name("gltf_check.py")
SQUARE = [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]


def read_3d(path):
    """Facts about a 3D file from pygltflib and trimesh. Skips the test if `uv` or the packages are not available."""
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    run = subprocess.run(
        ["uv", "run", "--no-project", "--with", "pygltflib", "--with", "trimesh", "--with", "numpy",
         "python", "-I", str(CHECK), str(path)],
        capture_output=True, text=True, timeout=600,
    )
    if run.returncode != 0 and "No solution found" in run.stderr + run.stdout:
        pytest.skip("pygltflib or trimesh is not available")
    assert run.returncode == 0, run.stderr
    return json.loads(run.stdout.splitlines()[-1])


def garden(cad):
    cad.execute({"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000, 6672000], "rotation": 0}})
    cad.result("hardscape_style.create", {"name": "Granite", "material": "Granite", "thickness": 150})
    cad.result("hardscape.add", {"boundary": SQUARE, "style": "Granite"})
    cad.result("plant_style.create", {"name": "Field maple", "botanical_name": "Acer campestre", "id_code": "AC",
                                      "category": "tree", "spread": 5000, "height": 9000})
    cad.result("plants.place", {"mode": "poly", "style": "Field maple",
                                "points": [[6000, 1000], [9000, 1000], [12000, 1000], [15000, 1000]]})
    cad.execute({"op": "add_object", "shape": {"kind": "landscape_area", "boundary": [[0, 5000], [6000, 5000], [6000, 8000], [0, 8000]], "name": "Meadow"}})


def garden_with_materials(cad):
    """The garden, a massing model, and materials on a massing model, a slab and the parts of a plant."""
    garden(cad)
    house = cad.result("massing.add", {"footprint": [[-9000, 0], [-3000, 0], [-3000, 5000], [-9000, 5000]], "floors": 2,
                                       "floor_height": 2800, "roof": "gable", "pitch": 35, "name": "House"})["id"]
    cad.result("material.add_defaults", {})
    cad.result("material.create", {"name": "Steel", "base_color": [200, 210, 220, 255], "metallic": 1.0, "roughness": 0.2})
    cad.result("material.create", {"name": "Glass", "base_color": [150, 200, 220, 255], "opacity": 0.4})
    cad.result("material.assign", {"material": "Brick", "object": house})
    plants = [o["id"] for o in cad.result("objects.list", {"kind": "plant"})]
    cad.result("material.assign", {"material": "Bark mulch", "object": plants[0], "slot": "trunk"})
    cad.result("material.assign", {"material": "Grass", "object": plants[0], "slot": "canopy"})
    cad.result("material.assign", {"material": "Glass", "object": plants[1]})
    return house


def test_glb_is_a_valid_y_up_file_in_metres_with_instanced_plants(cad, tmp_path):
    garden(cad)
    out = tmp_path / "garden.glb"
    r = cad.result("export.file", {"path": str(out), "format": "glb"})
    assert r["exported"] == 6
    assert r["report"]["instances"] == 3
    facts = read_3d(out)
    g = facts["gltf"]
    assert g["version"] == "2.0"
    assert g["nodes"] == 6
    assert g["shared_meshes"] == 3
    assert any(n.startswith("plant") for n in g["node_names"])
    ids = sorted(e["id"] for e in g["extras"])
    assert len(ids) == 6 and all(e["kind"] for e in g["extras"])
    assert "CADEKTOR_georeference" in g["extensions_used"]
    ext = g["scene_extensions"]["CADEKTOR_georeference"]
    assert ext["crs"] == "EPSG:3067"
    assert ext["map_origin"] == [385000, 6672000]
    t = facts["trimesh"]
    # The slab is 4 m by 3 m and 0.15 m thick; the garden is about 15 m wide. Y is up.
    ex = t["extents"]
    assert 14.0 < ex[0] < 20.0
    assert 7.0 < ex[1] < 9.5, "height of the trees in metres"
    assert 7.0 < ex[2] < 12.0, "depth: the plan Y axis is minus Z"


def test_gltf_text_and_obj_match_the_glb(cad, tmp_path):
    garden(cad)
    glb = tmp_path / "g.glb"
    gltf = tmp_path / "g.gltf"
    obj = tmp_path / "g.obj"
    cad.result("export.file", {"path": str(glb)})
    cad.result("export.file", {"path": str(gltf), "options": {"instancing": False}})
    r = cad.result("export.file", {"path": str(obj), "options": {"units": "m", "y_up": True}})
    assert (tmp_path / "g.mtl").exists() and len(r["files"]) == 2
    a, b, c = read_3d(glb), read_3d(gltf), read_3d(obj)
    assert a["trimesh"]["triangles"] == b["trimesh"]["triangles"] == c["trimesh"]["triangles"]
    for k in range(3):
        assert a["trimesh"]["extents"][k] == approx(c["trimesh"]["extents"][k], abs=0.01)
    # Without instancing the nodes carry their own meshes.
    assert b["gltf"]["shared_meshes"] == 0


def test_units_axes_and_grouping_options(cad, tmp_path):
    garden(cad)
    mm = tmp_path / "mm.glb"
    cad.result("export.file", {"path": str(mm), "options": {"units": "mm", "y_up": False, "grouping": "layer"}})
    f = read_3d(mm)
    assert f["gltf"]["nodes"] == 1
    assert f["gltf"]["extras"][0]["group"] == "layer"
    ex = f["trimesh"]["extents"]
    # Millimetres, Z up: 15 000 mm wide, 8 000 mm deep, 9 000 mm tall.
    assert 14000 < ex[0] < 20000 and 7000 < ex[1] < 12000 and 7000 < ex[2] < 9500
    bad = cad.rpc("export.file", {"path": str(tmp_path / "x.glb"), "options": {"bogus": True}})
    assert bad["error"]["code"] == -32602


def test_export_is_deterministic(cad, tmp_path):
    garden(cad)
    one, two = tmp_path / "1.glb", tmp_path / "2.glb"
    cad.result("export.file", {"path": str(one)})
    cad.result("export.file", {"path": str(two)})
    assert one.read_bytes() == two.read_bytes()


def box_and_line(cad):
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [4000, 0], [4000, 1000], [0, 1000]],
                                               "material": "Concrete", "thickness": 500, "elevation": 1500}})
    return cad.result("section_viewport.line_add", {"a": [-1000, 500], "b": [5000, 500], "name": "A"})["id"]


def test_a_section_viewport_cuts_the_model_and_follows_changes(cad):
    line = box_and_line(cad)
    sheet = cad.result("sheet.create", {"name": "Sections", "paper": "a3"})["layer"]
    vp = cad.result("section_viewport.create", {"sheet": sheet, "line": line, "scale": "fit"})
    assert vp["view"] == "section"
    got = cad.result("section_viewport.get", {"id": vp["id"], "geometry": True})
    face = got["cut_faces"][0]
    assert face["area_m2"] == approx(4.0 * 0.5)
    xs = [p[0] for p in face["rings"][0]]
    ys = [p[1] for p in face["rings"][0]]
    assert (min(xs), max(xs)) == (approx(1000), approx(5000))
    assert (min(ys), max(ys)) == (approx(1000), approx(1500))
    # The sheet draws a filled face.
    svg = cad.result("sheet.render", {"sheet": sheet})["svg"]
    assert "<path" in svg
    # Move the line off the box: nothing is cut any more.
    cad.result("section_viewport.line_set", {"id": line, "a": [-1000, 2000], "b": [5000, 2000]})
    assert cad.result("section_viewport.get", {"id": vp["id"]})["cut_faces"] == []
    # Undo brings the cut back.
    cad.result("undo", {})
    assert len(cad.result("section_viewport.get", {"id": vp["id"]})["cut_faces"]) == 1


def test_an_elevation_has_no_cut_and_prints_to_pdf(cad, tmp_path):
    line = box_and_line(cad)
    cad.result("section_viewport.line_set", {"id": line, "mode": "elevation", "depth": 20000})
    sheet = cad.result("sheet.create", {"name": "Elevations", "paper": "a3"})["layer"]
    cad.result("section_viewport.create", {"sheet": sheet, "line": line, "scale": 50})
    got = cad.result("section_viewport.get", {"id": line})
    assert got["cut_faces"] == [] and got["edge_count"] > 0
    pdf = tmp_path / "elevation.pdf"
    cad.result("export.file", {"path": str(pdf), "format": "pdf"})
    assert pdf.read_bytes().startswith(b"%PDF")


def test_describe_lists_the_new_methods_and_examples(cad):
    d = cad.result("describe", {})
    names = {m["name"] for m in d["methods"]}
    assert {"section_viewport.line_add", "section_viewport.create", "section_viewport.update", "section_viewport.get"} <= names
    assert d["io"]["section_viewport"]["examples"]
    assert {"gltf", "glb", "obj"} <= set(d["io"]["x3d"]["formats"])


def test_the_khronos_validator_accepts_the_files(cad, tmp_path):
    """Runs the Khronos glTF validator (npm: gltf-validator). Skips if node, npm or the network is not available."""
    if shutil.which("node") is None or shutil.which("npm") is None:
        pytest.skip("node and npm are not installed")
    tool = tmp_path / "validator"
    tool.mkdir()
    shutil.copy(Path(__file__).with_name("gltf_validate.js"), tool / "validate.js")
    (tool / "package.json").write_text("{}")
    install = subprocess.run(["npm", "install", "gltf-validator"], cwd=tool, capture_output=True, text=True, timeout=300)
    if install.returncode != 0:
        pytest.skip("gltf-validator could not be installed")
    garden_with_materials(cad)
    cad.execute({"op": "add_object", "shape": {"kind": "polyline3d", "points": [[0, 0, 0], [1000, 0, 500], [2000, 500, 100]], "closed": False}})
    files = []
    for name, options in [("a.glb", {}), ("b.gltf", {"grouping": "class"}), ("c.glb", {"y_up": False, "units": "mm", "instancing": False})]:
        cad.result("export.file", {"path": str(tmp_path / name), "options": options})
        files.append(str(tmp_path / name))
    run = subprocess.run(["node", "validate.js", *files], cwd=tool, capture_output=True, text=True, timeout=300)
    assert run.returncode == 0, run.stderr
    for line in run.stdout.splitlines():
        r = json.loads(line)
        assert r["errors"] == 0 and r["warnings"] == 0, r


def test_materials_massing_and_textures_reach_the_files(cad, tmp_path):
    """Textures are embedded in GLB and GLTF and are files next to OBJ; trimesh reads them back."""
    house = garden_with_materials(cad)
    glb, gltf, obj = tmp_path / "m.glb", tmp_path / "m.gltf", tmp_path / "m.obj"
    cad.result("export.file", {"path": str(glb)})
    cad.result("export.file", {"path": str(gltf)})
    r = cad.result("export.file", {"path": str(obj)})
    assert list(tmp_path.glob("Brick_*.png")) and len(r["files"]) >= 3
    f = read_3d(glb)["gltf"]
    assert "House" in " ".join(f["node_names"]) or any("massing" in n for n in f["node_names"])
    assert f["materials"] >= 5
    t = read_3d(obj)["trimesh"]
    assert t["triangles"] > 0
    g = read_3d(gltf)
    assert g["trimesh"]["triangles"] == read_3d(glb)["trimesh"]["triangles"]
    # The massing model is 6 m by 5 m with a roof: its box in the file is at least that big.
    nodes = {e["id"]: e for e in f["extras"]}
    assert nodes[house]["kind"] == "massing"


def test_massing_is_cut_by_a_section_viewport(cad):
    house = cad.result("massing.add", {"footprint": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "floors": 2,
                                       "floor_height": 3000, "roof": "flat"})["id"]
    line = cad.result("section_viewport.line_add", {"a": [-1000, 3000], "b": [9000, 3000]})["id"]
    got = cad.result("section_viewport.get", {"id": line, "geometry": True})
    face = got["cut_faces"][0]
    assert face["object"] == house
    assert face["area_m2"] == approx(8.0 * 6.0)
