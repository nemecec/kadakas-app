"""Scenario: source tracking of DXF imports (IOP-22). Revision pairs are made with ezdxf (move an
entity, change a layer, delete, add, renumber the handles) and the tags in the import result are
checked through the API: the handle, the source, the hash, the layers, blocks, save and copy."""

import re

import pytest

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")

HASH = re.compile(r"^[0-9a-f]{16}$")


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
    """All import tags in the document: {(source, handle, kind, via-handles): tag}."""
    out = {}
    for o in cad.result("objects.list", {}):
        t = o.get("data", {}).get("import")
        if t is not None:
            out[(t["source"], t.get("handle"), t["kind"])] = t
    return out


def by_handle(cad, source=None):
    return {
        t["handle"]: t
        for (src, _h, _k), t in tags(cad).items()
        if t.get("handle") and (source is None or src == source)
    }


def test_objects_carry_the_dxf_handle_the_source_and_a_hash(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    doc = build_v1(path)
    handles = {e.dxf.handle.upper(): e.dxftype() for e in doc.modelspace()}
    r = cad.result("import.file", {"path": str(path)})
    assert r["imported"] == 6
    source = r["report"]["source"]
    assert re.fullmatch(r"plan_v1\.dxf#[0-9a-f]{16}", source)
    found = by_handle(cad)
    assert {h: t["kind"] for h, t in found.items()} == handles
    for t in found.values():
        assert t["source"] == source
        assert HASH.match(t["hash"])
    # The tag is the only thing the importer adds to the data of a line.
    line = next(o for o in cad.result("objects.list", {"kind": "line"}))
    assert list(line["data"]) == ["import"]


def test_imported_objects_live_on_their_own_layer(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    design = cad.result("document.summary")["layers"][0]["id"]
    r = cad.result("import.file", {"path": str(path)})
    layers = cad.result("document.summary")["layers"]
    assert [la["name"] for la in layers] == ["Layer 1", "plan_v1.dxf"]
    own = layers[1]
    assert own["import_source"] == {"source": r["report"]["source"], "dxf_layer": ""}
    objs = cad.result("objects.list", {})
    assert objs and all(o["layer"] == own["id"] != design for o in objs)
    # The DXF layers are classes, as before.
    assert {"Walls", "Trees", "Paths"} <= {c["name"] for c in cad.result("document.summary")["classes"]}
    # Importing the same file again uses the same layer.
    cad.result("import.file", {"path": str(path)})
    assert len(cad.result("document.summary")["layers"]) == 2
    # Design work never lands on the layer of the import: the active layer is still the design layer.
    assert cad.result("document.summary")["active_layer"] == design


def test_layers_mode_keeps_the_dxf_layers_apart_from_design_layers(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    cad.result("execute", {"command": {"op": "add_layer", "name": "Walls"}})
    design_walls = next(la for la in cad.result("document.summary")["layers"] if la["name"] == "Walls")
    r = cad.result("import.file", {"path": str(path), "options": {"layer_mode": "layers"}})
    layers = {la["name"]: la for la in cad.result("document.summary")["layers"]}
    assert set(layers) >= {"Walls", "Walls (plan_v1.dxf)", "Trees", "Paths"}
    imported_walls = layers["Walls (plan_v1.dxf)"]
    assert imported_walls["import_source"] == {"source": r["report"]["source"], "dxf_layer": "Walls"}
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
    kept = {e.dxf.handle.upper() for e in msp} - {moved.dxf.handle.upper(), relayered.dxf.handle.upper(), deleted_handle}
    moved.translate(5, 0, 0)
    relayered.dxf.layer = "Trees"
    msp.delete_entity(deleted)
    added = msp.add_line((2000, 0), (3000, 0), dxfattribs={"layer": "Paths"})
    doc.saveas(str(v2))

    s1 = cad.result("import.file", {"path": str(v1)})["report"]["source"]
    s2 = cad.result("import.file", {"path": str(v2)})["report"]["source"]
    assert s1 != s2, "two files differ even when the name is alike up to the version"
    a, b = by_handle(cad, s1), by_handle(cad, s2)
    for h in kept:
        assert a[h]["hash"] == b[h]["hash"], h
    assert a[moved.dxf.handle.upper()]["hash"] != b[moved.dxf.handle.upper()]["hash"]
    assert a[relayered.dxf.handle.upper()]["hash"] != b[relayered.dxf.handle.upper()]["hash"]
    assert deleted_handle in a and deleted_handle not in b
    assert added.dxf.handle.upper() in b and added.dxf.handle.upper() not in a
    # Each revision has its own layer.
    assert [la["name"] for la in cad.result("document.summary")["layers"]] == ["Layer 1", "plan_v1.dxf", "plan_v2.dxf"]


def test_renumbered_handles_keep_the_content_hash(cad, tmp_path):
    """A program that exports the drawing again writes new handles. The hash does not use them."""
    v1, v3 = tmp_path / "plan_v1.dxf", tmp_path / "plan_v3.dxf"
    build_v1(v1)
    src = ezdxf.readfile(str(v1))
    doc = new_doc()
    msp = doc.modelspace()
    # Same entities, written in the other order, so every handle is new.
    for e in reversed(list(src.modelspace())):
        msp.add_foreign_entity(e.copy())
    doc.saveas(str(v3))
    s1 = cad.result("import.file", {"path": str(v1)})["report"]["source"]
    s3 = cad.result("import.file", {"path": str(v3)})["report"]["source"]
    a, b = by_handle(cad, s1), by_handle(cad, s3)
    assert {t["hash"] for t in a.values()} == {t["hash"] for t in b.values()}
    assert len(a) == len(b) == 6
    handle_of = lambda found: {t["hash"]: h for h, t in found.items()}
    assert handle_of(a) != handle_of(b), "the handles changed"


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
    found = [o["data"]["import"] for o in cad.result("objects.list", {})]
    single = [t for t in found if t["via"][0]["handle"] == ref.dxf.handle.upper()]
    assert {t["handle"] for t in single} == {inner_line.dxf.handle.upper(), inner_circle.dxf.handle.upper()}
    for t in single:
        assert t["block"] == {"name": "TREE", "handle": block_handle}
        assert HASH.match(t["via"][0]["hash"])
        assert "copy" not in t["via"][0]
    array = [t for t in found if t["via"][0]["handle"] == grid.dxf.handle.upper()]
    assert sorted(t["via"][0]["copy"] for t in array) == [0, 0, 1, 1]


def test_tracking_can_be_turned_off_and_the_layer_can_be_named(cad, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    r = cad.result("import.file", {"path": str(path), "options": {"track_source": False}})
    assert "source" not in r["report"]
    assert len(cad.result("document.summary")["layers"]) == 1
    assert not tags(cad)
    cad.result("undo")
    r = cad.result("import.file", {"path": str(path), "options": {"layer": "Survey", "source_name": "survey.dxf"}})
    assert r["report"]["source"].startswith("survey.dxf#")
    names = [la["name"] for la in cad.result("document.summary")["layers"]]
    assert names == ["Layer 1", "Survey"]
    assert len(tags(cad)) == 6


def test_tags_survive_save_and_load_and_a_copy_is_new_work(cad, make_serve, tmp_path):
    path = tmp_path / "plan_v1.dxf"
    build_v1(path)
    cad.result("import.file", {"path": str(path)})
    saved = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(saved)})
    other = make_serve()
    other.result("file.open", {"path": str(saved)})
    assert tags(other) == tags(cad) and len(tags(other)) == 6
    assert any("import_source" in la for la in other.result("document.summary")["layers"])
    # Copy an imported line: the copy has no tag, and the original keeps it.
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
    err = cad.error("execute", {"command": {"op": "set_data", "id": line["id"], "set": {"import": {"source": ""}}}})
    assert "import tag" in err["message"]
    bad_layer = cad.error(
        "execute", {"command": {"op": "set_layer_source", "id": line["layer"], "source": {"source": ""}}}
    )
    assert "layer source" in bad_layer["message"]
    # A document file with a bad tag opens, and the tag is dropped.
    saved = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(saved)})
    text = saved.read_text().replace('"kind": "CIRCLE"', '"kind": ""')
    assert text != saved.read_text()
    saved.write_text(text)
    cad.result("file.open", {"path": str(saved)})
    assert len(tags(cad)) == 5


def test_a_dxf_with_no_handles_still_gets_hashes(cad, tmp_path):
    path = tmp_path / "old.dxf"
    path.write_text("0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n0\nENDSEC\n0\nEOF\n")
    cad.result("import.file", {"path": str(path)})
    (line,) = cad.result("objects.list", {"kind": "line"})
    t = line["data"]["import"]
    assert "handle" not in t and HASH.match(t["hash"]) and t["kind"] == "LINE"
