"""Scenario: source tracking of DXF imports (IOP-22). Revision pairs are made with ezdxf (move an
entity, change a layer, delete, add, renumber the handles, rename the file, save again) and the
tags in the import result are checked through the API: the handle, the drawing and revision, the
hashes, the layers, blocks, the stored base, save and copy."""

import re

import pytest

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")

HASH = re.compile(r"^[0-9a-f]{16}$")
GUID = re.compile(r"^guid:\{[0-9A-F-]+\}$")


def new_doc():
    doc = ezdxf.new("R2018", setup=True)
    doc.header["$INSUNITS"] = 4
    for name in ("Walls", "Trees", "Paths"):
        doc.layers.add(name)
    return doc


def build_v1(path):
    """A small drawing: four lines, a circle and a text, each on a layer."""
    doc = new_doc()
    msp = doc.modelspace()
    msp.add_line((0, 0), (1000, 0), dxfattribs={"layer": "Walls"})
    msp.add_line((1000, 0), (1000, 1000), dxfattribs={"layer": "Walls"})
    msp.add_line((1000, 1000), (0, 1000), dxfattribs={"layer": "Walls"})
    msp.add_line((0, 1000), (0, 0), dxfattribs={"layer": "Paths"})
    msp.add_circle((500, 500), 50, dxfattribs={"layer": "Trees"})
    msp.add_text("Oak", height=100, dxfattribs={"layer": "Trees", "insert": (600, 600)})
    doc.saveas(str(path))
    return doc


def tags(cad):
    """All import tags in the document, in the order of the objects."""
    return [o["data"]["import"] for o in cad.result("objects.list", {}) if "import" in o.get("data", {})]


def by_handle(cad, revision=None):
    return {t["handle"]: t for t in tags(cad) if t.get("handle") and (revision is None or t["revision"] == revision)}


def layers(cad):
    return cad.result("document.summary")["layers"]


def test_objects_carry_the_dxf_handle_drawing_revision_and_hashes(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    doc = build_v1(path)
    handles = {e.dxf.handle.upper(): e.dxftype() for e in doc.modelspace()}
    r = cad.result("import.file", {"path": str(path)})
    assert r["imported"] == 6
    drawing, revision = r["report"]["drawing"], r["report"]["revision"]
    assert drawing == "guid:" + doc.header["$FINGERPRINTGUID"]
    assert revision == "guid:" + doc.header["$VERSIONGUID"]
    found = by_handle(cad)
    assert {h: t["kind"] for h, t in found.items()} == handles
    for t in found.values():
        assert (t["drawing"], t["revision"]) == (drawing, revision)
        assert HASH.match(t["hash"]) and HASH.match(t["obj"])
    line = next(o for o in cad.result("objects.list", {"kind": "line"}))
    assert list(line["data"]) == ["import"]


def test_imported_objects_live_on_their_own_layer_with_the_transform(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    design = layers(cad)[0]["id"]
    r = cad.result("import.file", {"path": str(path)})
    ls = layers(cad)
    assert [la["name"] for la in ls] == ["Layer 1", "plan_v1.dxf"]
    own = ls[1]
    assert own["import_source"] == {
        "drawing": r["report"]["drawing"],
        "revision": r["report"]["revision"],
        "dxf_layer": "",
        "unit_scale": 1.0,
        "shift": [0.0, 0.0],
    }
    objs = cad.result("objects.list", {})
    assert objs and all(o["layer"] == own["id"] != design for o in objs)
    assert {"Walls", "Trees", "Paths"} <= {c["name"] for c in cad.result("document.summary")["classes"]}
    assert cad.result("document.summary")["active_layer"] == design
    # The summaries of the entities of this revision are kept in the document.
    (base,) = cad.result("resource.list", {"type": "import_base"})["resources"]
    assert base["type"] == "import_base"


def test_the_same_file_again_reuses_the_layer_and_says_so(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    cad.result("import.file", {"path": str(path)})
    r = cad.result("import.file", {"path": str(path)})
    assert len(layers(cad)) == 2
    assert any("use Update" in n for n in r["report"]["notes"])
    # The same revision keeps one base.
    assert len(cad.result("resource.list", {"type": "import_base"})["resources"]) == 1


def test_a_resave_and_a_rename_keep_the_drawing_and_reuse_its_layer(cad, tmp_path):
    v1, v2 = tmp_path / "plan_v1.dxf", tmp_path / "plan_final.dxf"
    build_v1(v1)
    doc = ezdxf.readfile(str(v1))
    doc.saveas(str(v2))  # a new $VERSIONGUID, the same $FINGERPRINTGUID, another name
    r1 = cad.result("import.file", {"path": str(v1)})["report"]
    r2 = cad.result("import.file", {"path": str(v2)})["report"]
    assert GUID.match(r1["drawing"]) and r1["drawing"] == r2["drawing"]
    assert r1["revision"] != r2["revision"]
    ls = layers(cad)
    assert [la["name"] for la in ls] == ["Layer 1", "plan_v1.dxf"], "the layer is reused"
    assert ls[1]["import_source"]["revision"] == r2["revision"]
    assert len(tags(cad)) == 12
    assert len(cad.result("resource.list", {"type": "import_base"})["resources"]) == 2


def test_layers_mode_keeps_the_dxf_layers_apart_from_design_layers(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    cad.result("execute", {"command": {"op": "add_layer", "name": "Walls"}})
    design_walls = next(la for la in layers(cad) if la["name"] == "Walls")
    r = cad.result("import.file", {"path": str(path), "options": {"layer_mode": "layers"}})
    by_name = {la["name"]: la for la in layers(cad)}
    assert set(by_name) >= {"Walls", "Walls (plan_v1.dxf)", "Trees", "Paths"}
    imported_walls = by_name["Walls (plan_v1.dxf)"]
    assert imported_walls["import_source"]["dxf_layer"] == "Walls"
    assert imported_walls["import_source"]["drawing"] == r["report"]["drawing"]
    assert "import_source" not in design_walls
    assert not [o for o in cad.result("objects.list", {}) if o["layer"] == design_walls["id"]]
    assert len([o for o in cad.result("objects.list", {}) if o["layer"] == imported_walls["id"]]) == 3


def test_a_revision_keeps_handles_and_changes_the_hash_of_an_edited_entity(cad, tmp_path):
    v1, v2 = tmp_path / "plan_v1.dxf", tmp_path / "plan_v2.dxf"
    build_v1(v1)
    doc = ezdxf.readfile(str(v1))
    msp = doc.modelspace()
    lines = list(msp.query("LINE"))
    moved, relayered, deleted = lines[0], lines[1], lines[2]
    deleted_handle = deleted.dxf.handle.upper()
    kept = {e.dxf.handle.upper() for e in msp} - {
        moved.dxf.handle.upper(),
        relayered.dxf.handle.upper(),
        deleted_handle,
    }
    moved.translate(5, 0, 0)
    relayered.dxf.layer = "Trees"
    msp.delete_entity(deleted)
    added = msp.add_line((2000, 0), (3000, 0), dxfattribs={"layer": "Paths"})
    doc.saveas(str(v2))

    r1 = cad.result("import.file", {"path": str(v1)})["report"]
    r2 = cad.result("import.file", {"path": str(v2)})["report"]
    assert r1["drawing"] == r2["drawing"] and r1["revision"] != r2["revision"]
    a, b = by_handle(cad, r1["revision"]), by_handle(cad, r2["revision"])
    for h in kept:
        assert a[h]["hash"] == b[h]["hash"], h
    assert a[moved.dxf.handle.upper()]["hash"] != b[moved.dxf.handle.upper()]["hash"]
    assert a[relayered.dxf.handle.upper()]["hash"] != b[relayered.dxf.handle.upper()]["hash"]
    assert deleted_handle in a and deleted_handle not in b
    assert added.dxf.handle.upper() in b and added.dxf.handle.upper() not in a


def test_renumbered_handles_keep_the_content_hash(cad, tmp_path):
    """A program that exports the drawing again writes new handles. The hash does not use them."""
    v1, v3 = tmp_path / "plan_v1.dxf", tmp_path / "plan_v3.dxf"
    build_v1(v1)
    src = ezdxf.readfile(str(v1))
    doc = new_doc()
    msp = doc.modelspace()
    for e in reversed(list(src.modelspace())):
        msp.add_foreign_entity(e.copy())
    doc.saveas(str(v3))
    r1 = cad.result("import.file", {"path": str(v1)})["report"]
    r3 = cad.result("import.file", {"path": str(v3)})["report"]
    assert r1["drawing"] != r3["drawing"], "another program, another fingerprint"
    a, b = by_handle(cad, r1["revision"]), by_handle(cad, r3["revision"])
    assert {t["hash"] for t in a.values()} == {t["hash"] for t in b.values()}
    assert len(a) == len(b) == 6
    handle_of = lambda found: {t["hash"]: h for h, t in found.items()}
    assert handle_of(a) != handle_of(b), "the handles changed"


def test_files_without_a_header_with_the_same_name_are_two_drawings(cad, tmp_path):
    one, two = tmp_path / "a" / "old.dxf", tmp_path / "b" / "old.dxf"
    for p, y in ((one, 5), (two, 9)):
        p.parent.mkdir()
        p.write_text(f"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n{y}\n0\nENDSEC\n0\nEOF\n")
    r1 = cad.result("import.file", {"path": str(one)})["report"]
    r2 = cad.result("import.file", {"path": str(two)})["report"]
    assert r1["drawing"] != r2["drawing"]
    assert any("no header" in n and "confirm" in n for n in r1["notes"])
    assert len(layers(cad)) == 3
    (line, _other) = sorted(cad.result("objects.list", {"kind": "line"}), key=lambda o: o["data"]["import"]["hash"])
    t = line["data"]["import"]
    assert "handle" not in t and HASH.match(t["hash"]) and t["kind"] == "LINE"


def test_block_references_and_definitions_keep_their_handles(cad, tmp_path):
    path = tmp_path / "blocks.dxf"
    doc = new_doc()
    blk = doc.blocks.new("TREE")
    inner_line = blk.add_line((0, 0), (100, 0))
    inner_circle = blk.add_circle((0, 0), 30)
    msp = doc.modelspace()
    ref = msp.add_blockref("TREE", (1000, 0), dxfattribs={"layer": "Trees"})
    grid = msp.add_blockref("TREE", (0, 1000), dxfattribs={"layer": "Trees"})
    grid.dxf.column_count, grid.dxf.column_spacing = 2, 500
    doc.saveas(str(path))
    block_handle = doc.blocks.get("TREE").block.dxf.handle.upper()
    r = cad.result("import.file", {"path": str(path)})
    assert r["imported"] == 6
    found = tags(cad)
    single = [t for t in found if t["via"][0]["handle"] == ref.dxf.handle.upper()]
    assert {t["handle"] for t in single} == {inner_line.dxf.handle.upper(), inner_circle.dxf.handle.upper()}
    for t in single:
        assert t["block"]["name"] == "TREE" and t["block"]["handle"] == block_handle
        assert t["block"]["key"].startswith("rec:"), t["block"]
        assert HASH.match(t["via"][0]["hash"])
        assert "copy" not in t["via"][0]
    array = [t for t in found if t["via"][0]["handle"] == grid.dxf.handle.upper()]
    assert sorted(t["via"][0]["copy"] for t in array) == [0, 0, 1, 1]


def test_tracking_can_be_turned_off_and_the_layer_can_be_named(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    r = cad.result("import.file", {"path": str(path), "options": {"track_source": False}})
    assert "drawing" not in r["report"] and "revision" not in r["report"]
    assert len(layers(cad)) == 1
    assert not tags(cad)
    assert not cad.result("resource.list", {"type": "import_base"})["resources"]
    cad.result("undo")
    cad.result("import.file", {"path": str(path), "options": {"layer": "Survey"}})
    assert [la["name"] for la in layers(cad)] == ["Layer 1", "Survey"]
    assert len(tags(cad)) == 6


def test_a_file_over_the_size_limit_is_refused(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    err = cad.error("import.file", {"path": str(path), "options": {"max_bytes": 100}})
    assert "max_bytes" in err["message"]
    assert not cad.result("objects.list", {})


def test_tags_survive_save_and_load_and_a_copy_is_new_work(cad, make_serve, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    cad.result("import.file", {"path": str(path)})
    saved = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(saved)})
    other = make_serve()
    other.result("file.open", {"path": str(saved)})
    assert tags(other) == tags(cad) and len(tags(other)) == 6
    assert any("import_source" in la for la in layers(other))
    assert len(other.result("resource.list", {"type": "import_base"})["resources"]) == 1
    line = next(iter(other.result("objects.list", {"kind": "line"})))
    out = other.result("execute", {"command": {"op": "copy_objects", "ids": [line["id"]], "delta": [10, 10]}})
    copy_id = out["created"][0]
    objs = {o["id"]: o for o in other.result("objects.list", {})}
    assert "import" not in objs[copy_id].get("data", {})
    assert "import" in objs[line["id"]]["data"]
    assert objs[copy_id]["layer"] == objs[line["id"]]["layer"]


def test_a_tag_that_is_not_valid_is_refused(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    cad.result("import.file", {"path": str(path)})
    line = next(iter(cad.result("objects.list", {"kind": "line"})))
    err = cad.error("execute", {"command": {"op": "set_data", "id": line["id"], "set": {"import": {"drawing": ""}}}})
    assert "import tag" in err["message"]
    bad_layer = cad.error(
        "execute", {"command": {"op": "set_layer_source", "id": line["layer"], "source": {"drawing": ""}}}
    )
    assert bad_layer["code"] != 0
    # A document file with a bad tag opens, and the tag is dropped.
    saved = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(saved)})
    text = saved.read_text().replace('"kind": "CIRCLE"', '"kind": ""')
    assert text != saved.read_text()
    saved.write_text(text)
    cad.result("file.open", {"path": str(saved)})
    assert len(tags(cad)) == 5


def test_the_import_says_which_update_data_it_kept(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    # The default keeps the summaries of the entities in the document.
    first = cad.result("import.file", {"path": str(path)})
    assert first["base"] == "auto" and first["report"]["base"] == "auto"
    assert "link" not in imported_source(cad)


def imported_source(cad):
    """The import source of the first imported layer."""
    return next(l["import_source"] for l in layers(cad) if "import_source" in l)


def test_base_link_keeps_the_path_size_and_hash_and_survives_save_and_load(cad, make_serve, tmp_path):
    import hashlib

    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    r = cad.result("import.file", {"path": str(path), "options": {"base": "link"}})
    assert r["base"] == "link"
    source = imported_source(cad)
    data = path.read_bytes()
    assert source["link"] == {
        "path": str(path.resolve()),
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }
    saved = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(saved)})
    other = make_serve(saved)
    assert imported_source(other)["link"] == source["link"]


def test_base_none_imports_without_update_data(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    r = cad.result("import.file", {"path": str(path), "options": {"base": "none"}})
    assert r["base"] == "none"
    assert len(tags(cad)) == 6, "the objects keep their tags"
    assert "link" not in imported_source(cad)


def test_a_bad_base_value_and_a_bad_link_path_are_refused(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    err = cad.error("import.file", {"path": str(path), "options": {"base": "sometimes"}})
    assert "sometimes" in err["message"]
    cad.result("import.file", {"path": str(path)})
    layer = next(l["id"] for l in layers(cad) if "import_source" in l)
    source = imported_source(cad)
    for bad in ("relative/plan.dxf", "/data/../etc/passwd", "/data/\u0007plan.dxf"):
        linked = {**source, "link": {"path": bad, "bytes": 1, "sha256": "0" * 64}}
        err = cad.error("execute", {"command": {"op": "set_layer_source", "id": layer, "source": linked}})
        assert err["code"] != 0, bad
