"""Scenario: an architect sends revisions of a drawing and the landscape designer updates (IOP-23)
and compares drawings (IOP-24). Revision pairs are made with ezdxf the way a CAD program changes a
drawing: move an entity, change a layer, delete, add, edit a text, change a block definition,
export again with new handles. The answers of `import.update`, `import.apply` and `dxf.compare`
are checked through the API."""

import base64
import struct
import zlib

import pytest

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")

CLOUD = (150, 40, 190)


def new_doc():
    doc = ezdxf.new("R2018", setup=True)
    doc.header["$INSUNITS"] = 4
    for name in ("Walls", "Trees", "Paths", "Notes"):
        doc.layers.add(name)
    return doc


def build_v1(path):
    """A site plan: a fence of four lines, a path, trees as block references, and two notes."""
    doc = new_doc()
    tree = doc.blocks.new("TREE")
    tree.add_circle((0, 0), 400)
    tree.add_line((-400, 0), (400, 0))
    msp = doc.modelspace()
    for a, b in (((0, 0), (20000, 0)), ((20000, 0), (20000, 12000)), ((20000, 12000), (0, 12000)), ((0, 12000), (0, 0))):
        msp.add_line(a, b, dxfattribs={"layer": "Walls"})
    msp.add_lwpolyline([(1000, 6000), (9000, 6500), (19000, 6000)], dxfattribs={"layer": "Paths"})
    for i in range(3):
        msp.add_blockref("TREE", (3000 + i * 5000, 3000), dxfattribs={"layer": "Trees"})
    msp.add_text("Oak", height=300, dxfattribs={"layer": "Notes", "insert": (3000, 3800)})
    msp.add_text("Gate", height=300, dxfattribs={"layer": "Notes", "insert": (19000, 400)})
    doc.saveas(str(path))
    return doc


def revise(src, dst):
    """The next revision: the east fence moves, the path changes its layer, a text is edited, the
    gate note goes, a new tree is planted. The tree block gets a bigger crown."""
    doc = ezdxf.readfile(str(src))
    msp = doc.modelspace()
    lines = list(msp.query("LINE[layer=='Walls']"))
    lines[1].translate(0, 40, 0)
    path = next(iter(msp.query("LWPOLYLINE")))
    path.dxf.layer = "Walls"
    texts = {t.dxf.text: t for t in msp.query("TEXT")}
    texts["Oak"].dxf.text = "Pedunculate oak"
    msp.delete_entity(texts["Gate"])
    msp.add_blockref("TREE", (18000, 9000), dxfattribs={"layer": "Trees"})
    circle = next(iter(doc.blocks.get("TREE").query("CIRCLE")))
    circle.dxf.radius = 700
    doc.saveas(str(dst))
    return doc


def tags(cad):
    return [o["data"]["import"] for o in cad.result("objects.list", {}) if "import" in o.get("data", {})]


def versions(cad):
    return [v["name"] for v in cad.result("version.list", {"current": False})["versions"]]


def shapes(cad):
    return sorted(
        (o["shape"]["kind"], o["id"]) for o in cad.result("objects.list", {})
    )


def import_v1(cad, tmp_path, **options):
    v1 = tmp_path / "site_v1.dxf"
    build_v1(v1)
    cad.result("import.file", {"path": str(v1), "options": options})
    return v1


def update(cad, path, **params):
    return cad.result("import.update", {"path": str(path), **params})


def kinds(result):
    return sorted((c["status"], c["entity"]) for c in result["changes"])


def test_a_revision_is_listed_and_applied_as_one_undo_step(cad, tmp_path):
    v1 = import_v1(cad, tmp_path)
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    before = cad.result("document.get")["objects"]
    depth_before = cad.result("objects.list", {})
    r = update(cad, v2)
    assert r["old_from"] == "base"
    assert r["drawing"] == tags(cad)[0]["drawing"]
    counts = r["counts"]
    assert (counts["added"], counts["removed"], counts["conflicts"]) == (1, 1, 0)
    assert counts["changed"] == 4, kinds(r)
    seen = kinds(r)
    assert ("removed", "TEXT") in seen
    assert ("added", "INSERT") in seen
    assert any(c["entity"] == "TEXT" and c["what"] == ["text"] for c in r["changes"] if c["status"] == "changed")
    text = next(c for c in r["changes"] if c["status"] == "changed" and c["entity"] == "TEXT")
    assert text["text"] == {"old": "Oak", "new": "Pedunculate oak"}
    # The tree block changed: every instance of it changed, so one change has 3 objects (2 parts).
    block_change = next(c for c in r["changes"] if c["entity"] == "CIRCLE")
    assert block_change["objects"] == 3
    # Nothing changed in the document yet.
    assert cad.result("document.get")["objects"] == before
    assert len(cad.result("objects.list", {})) == len(depth_before)

    applied = cad.result("import.apply", {"update_id": r["update_id"], "accept": "all"})
    assert applied["versions"] == {"before": 1, "after": 2}
    assert versions(cad) == ["Before update to site_v2.dxf", "After update to site_v2.dxf"]
    objs = cad.result("objects.list", {})
    texts = [o["shape"]["text"] for o in objs if o["shape"]["kind"] == "text"]
    assert texts == ["Pedunculate oak"]
    circles = [o["shape"]["radius"] for o in objs if o["shape"]["kind"] == "circle"]
    assert sorted(circles) == [700, 700, 700, 700]
    layer_names = {l["name"] for l in cad.result("document.summary")["layers"]}
    assert len(layer_names) == 2, "the objects stay on the layer of the drawing"

    # One undo step takes it all back, the versions too.
    assert cad.result("undo")["done"]
    assert cad.result("document.get")["objects"] == before
    assert versions(cad) == []
    assert cad.result("redo")["done"]
    assert len(versions(cad)) == 2

    # The next revision starts from this one.
    again = update(cad, v2)
    assert again["changes"] == [] and again["counts"]["unchanged"] > 0


def test_a_program_that_renumbers_the_handles_still_matches_the_entities(cad, tmp_path):
    v1 = import_v1(cad, tmp_path)
    src = ezdxf.readfile(str(v1))
    # Another program re-exports the drawing: another fingerprint, new handles, reversed order,
    # one wall moved a little.
    doc = new_doc()
    block = doc.blocks.new("TREE")
    for e in src.blocks.get("TREE"):
        block.add_foreign_entity(e.copy())
    msp = doc.modelspace()
    entities = list(src.modelspace())
    for e in reversed(entities):
        if e.dxftype() == "INSERT":
            msp.add_blockref(e.dxf.name, e.dxf.insert, dxfattribs={"layer": e.dxf.layer})
        else:
            msp.add_foreign_entity(e.copy())
    wall = next(iter(msp.query("LINE")))
    wall.translate(0, 30, 0)
    v3 = tmp_path / "site_export.dxf"
    doc.saveas(str(v3))
    drawing = tags(cad)[0]["drawing"]
    r = update(cad, v3, drawing=drawing, tolerance=200)
    assert r["match"]["renumbered"] is False or r["counts"]["changed"] >= 1
    assert r["counts"]["removed"] == 0 and r["counts"]["added"] == 0, kinds(r)
    assert r["counts"]["changed"] == 1
    assert any("another" in n or "content" in n for n in r["notes"]) or r["match"]["renumbered"] is False
    applied = cad.result("import.apply", {"update_id": r["update_id"], "accept": "all"})
    assert applied["applied"]["replaced"] == 1
    # Without naming the drawing, a file of another lineage is refused when it could be any.
    other = tmp_path / "other.dxf"
    build_v1(other)
    cad.result("import.file", {"path": str(other)})
    err = cad.error("import.update", {"path": str(v3)})
    assert "drawing" in err["message"]


def test_changes_the_person_made_are_conflicts_and_are_kept(cad, tmp_path):
    v1 = import_v1(cad, tmp_path)
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    # The person moves the oak text (the revision edits it) and a tree (the revision changes its block).
    objs = cad.result("objects.list", {})
    text = next(o for o in objs if o["shape"]["kind"] == "text")
    crown = next(o for o in objs if o["shape"]["kind"] == "circle")
    cad.execute({"op": "move_objects", "ids": [text["id"], crown["id"]], "delta": [0, 500]})
    r = update(cad, v2)
    assert r["counts"]["conflicts"] == 2
    assert len(r["conflicts"]) == 2
    flagged = [c for c in r["changes"] if c["conflict"]]
    assert {c["entity"] for c in flagged} == {"TEXT", "CIRCLE"}
    assert all(c["edited_ids"] for c in flagged)
    applied = cad.result("import.apply", {"update_id": r["update_id"], "accept": "all"})
    assert sorted(applied["skipped_conflicts"]) == sorted(r["conflicts"])
    objs = cad.result("objects.list", {})
    assert [o["shape"]["text"] for o in objs if o["shape"]["kind"] == "text"] == ["Oak"]
    moved = {o["id"]: o for o in objs}
    assert moved[crown["id"]]["shape"]["radius"] == 400, "the edited crown keeps its old size"
    assert sorted(o["shape"]["radius"] for o in objs if o["shape"]["kind"] == "circle") == [400, 700, 700, 700]

    # Overwriting is a choice: name the conflicts, or ask for overwrite.
    cad.result("undo")
    r = update(cad, v2)
    applied = cad.result("import.apply", {"update_id": r["update_id"], "accept": "all", "conflicts": "overwrite"})
    assert applied["skipped_conflicts"] == []
    texts = [o["shape"]["text"] for o in cad.result("objects.list", {}) if o["shape"]["kind"] == "text"]
    assert texts == ["Pedunculate oak"]


def test_accepting_some_changes_and_rejecting_others(cad, tmp_path):
    v1 = import_v1(cad, tmp_path)
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    r = update(cad, v2)
    added = [c["id"] for c in r["changes"] if c["status"] == "added"]
    removed = [c["id"] for c in r["changes"] if c["status"] == "removed"]
    n = len(cad.result("objects.list", {}))
    applied = cad.result("import.apply", {"update_id": r["update_id"], "accept": added + removed, "reject": removed})
    assert applied["applied"]["added"] == 2 and applied["applied"]["removed"] == 0
    assert applied["applied"]["replaced"] == 0
    assert len(cad.result("objects.list", {})) == n + 2


def test_objects_of_other_sources_are_never_touched(cad, tmp_path):
    v1 = import_v1(cad, tmp_path)
    cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [1, 1], "radius": 5}})
    # Another drawing with the same layer names and the same coordinates.
    other = tmp_path / "survey.dxf"
    build_v1(other)
    cad.result("import.file", {"path": str(other)})
    mine = {o["id"] for o in cad.result("objects.list", {}) if "import" not in o.get("data", {}) or
            o["data"]["import"]["drawing"] != tags(cad)[0]["drawing"]}
    snapshot = {o["id"]: o for o in cad.result("objects.list", {}) if o["id"] in mine}
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    drawing = tags(cad)[0]["drawing"]
    r = update(cad, v2, drawing=drawing)
    cad.result("import.apply", {"update_id": r["update_id"], "accept": "all"})
    now = {o["id"]: o for o in cad.result("objects.list", {})}
    for oid, obj in snapshot.items():
        if obj.get("data", {}).get("import", {}).get("drawing") != drawing:
            assert now[oid] == obj


def test_the_linked_original_is_read_checked_and_replaced_by_the_old_file_when_gone(cad, tmp_path):
    v1 = import_v1(cad, tmp_path, base="link")
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    copy = tmp_path / "site_v1_copy.dxf"
    copy.write_bytes(v1.read_bytes())
    # Present and unchanged: the link is the old side.
    r = update(cad, v2, dry_run=True)
    assert r["old_from"] == "link"
    # Changed since the import: told, and the old file is asked for.
    v1.write_bytes(v1.read_bytes() + b"\n")
    err = cad.error("import.update", {"path": str(v2)})
    assert "changed" in err["message"] and "old file" in err["message"]
    # Gone: the same.
    v1.unlink()
    err = cad.error("import.update", {"path": str(v2)})
    assert "gone" in err["message"] or "No such file" in err["message"]
    assert "old file" in err["message"] or "`old`" in err["message"]
    # The old file given by the caller.
    r = update(cad, v2, old=str(copy))
    assert r["old_from"] == "file"
    assert r["counts"]["changed"] == 4
    applied = cad.result("import.apply", {"update_id": r["update_id"], "accept": "all"})
    assert applied["applied"]["added"] == 2


def test_the_expiry_and_the_checks_of_the_update(cad, tmp_path):
    v1 = import_v1(cad, tmp_path)
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    r = update(cad, v2)
    cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [1, 1], "radius": 5}})
    err = cad.error("import.apply", {"update_id": r["update_id"], "accept": "all"})
    assert "import.update" in err["message"]
    assert cad.error("import.update", {"path": str(tmp_path / "missing.dxf")})
    assert cad.error("import.update", {"path": str(v2), "limit": 0})
    r = update(cad, v2)
    assert cad.error("import.apply", {"update_id": "u99", "accept": "all"})
    assert cad.error("import.apply", {"update_id": r["update_id"], "accept": ["nope"]})
    assert cad.error("import.apply", {"update_id": r["update_id"]})


def test_a_big_revision_lists_a_bounded_number_of_changes(cad, tmp_path):
    def make(path, shift):
        doc = new_doc()
        msp = doc.modelspace()
        for i in range(400):
            msp.add_line((0, i * 100), (5000, i * 100 + shift), dxfattribs={"layer": "Walls"})
        doc.header["$FINGERPRINTGUID"] = "{11111111-1111-1111-1111-111111111111}"
        doc.saveas(str(path))
    big1, big2 = tmp_path / "big1.dxf", tmp_path / "big2.dxf"
    make(big1, 0)
    make(big2, 40)
    cad.result("import.file", {"path": str(big1)})
    r = update(cad, big2, limit=25)
    assert r["total_changes"] == 400 and r["listed"] == 25 and len(r["changes"]) == 25
    assert any("25 of 400" in n for n in r["notes"])
    ids = [c["id"] for c in update(cad, big2, limit=1000)["changes"]]
    assert len(set(ids)) == 400, "change IDs are unique"
    assert ids[:25] == [c["id"] for c in r["changes"]], "and stable"
    assert update(cad, big2, layers=["Nothing"])["total_changes"] == 0


# -- Compare Drawings (IOP-24) ------------------------------------------------------------------


def decode_png(data):
    """RGBA pixels of a PNG that the renderer wrote (8 bits, no interlace)."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, header = 8, b"", None
    while pos < len(data):
        (n,) = struct.unpack(">I", data[pos:pos + 4])
        kind, body = data[pos + 4:pos + 8], data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    w, h, depth, color, _, _, interlace = header
    assert depth == 8 and interlace == 0 and color in (2, 6)
    bpp = 4 if color == 6 else 3
    raw = zlib.decompress(idat)
    rows, prev, i = [], bytearray(w * bpp), 0
    for _ in range(h):
        f, line = raw[i], bytearray(raw[i + 1:i + 1 + w * bpp])
        i += 1 + w * bpp
        for x in range(len(line)):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        rows.append(line)
        prev = line
    return w, h, bpp, rows


def nearest(png, rgb):
    _, _, bpp, rows = png
    best = 1e9
    for row in rows:
        for x in range(0, len(row), bpp):
            d = sum((row[x + k] - rgb[k]) ** 2 for k in range(3)) ** 0.5
            best = min(best, d)
    return best


def test_two_files_are_compared_without_an_import(cad, tmp_path):
    v1 = tmp_path / "site_v1.dxf"
    build_v1(v1)
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    r = cad.result("dxf.compare", {"base": str(v1), "new": str(v2)})
    assert r["counts"]["added"] == 1 and r["counts"]["removed"] == 1
    assert r["counts"]["changed"] == 4
    assert "image" not in r
    assert cad.result("objects.list", {}) == [], "nothing was imported"
    assert cad.result("document.summary")["objects"] == 0 if "objects" in cad.result("document.summary") else True
    entity = {(c["status"], c["entity"]) for c in r["changes"]}
    assert ("added", "INSERT") in entity and ("removed", "TEXT") in entity


def test_the_overlay_picture_has_the_tints_and_optional_revision_clouds(cad, tmp_path):
    v1 = tmp_path / "site_v1.dxf"
    build_v1(v1)
    v2 = tmp_path / "site_v2.dxf"
    revise(v1, v2)
    green, orange, red = (0x2E, 0xA0, 0x43), (0xE6, 0x7E, 0x22), (0xD6, 0x28, 0x28)
    png = cad.result(
        "dxf.compare",
        {"base": str(v1), "new": str(v2), "render": {"format": "png", "width": 600, "height": 400}},
    )
    legend = {row["status"]: row["color"] for row in png["compare"]["legend"]}
    assert set(legend) == {"added", "removed", "changed"}
    pixels = decode_png(base64.b64decode(png["image"]["png_base64"]))
    assert pixels[:2] == (600, 400)
    for status in ("added", "changed"):
        assert nearest(pixels, tuple(legend[status])) < 70, status
    assert nearest(pixels, tuple(legend["removed"])) < 110, "the ghost is see-through"
    assert nearest(pixels, CLOUD) > 70

    with_clouds = cad.result(
        "dxf.compare",
        {"base": str(v1), "new": str(v2), "render": {"format": "png", "width": 600, "height": 400, "revision_clouds": True}},
    )
    assert with_clouds["image"]["clouds"]["count"] >= 1
    assert nearest(decode_png(base64.b64decode(with_clouds["image"]["png_base64"])), CLOUD) < 70

    svg = cad.result("dxf.compare", {"base": str(v1), "new": str(v2), "render": {"format": "svg", "revision_clouds": True}})
    text = svg["image"]["svg"]
    assert text.startswith("<svg") or "<svg" in text
    assert 'data-mark="added">+</text>' in text and 'data-ghost="removed"' in text
    assert "rgba(150,40,190" in text

    pdf = cad.result("dxf.compare", {"base": str(v1), "new": str(v2), "render": {"format": "pdf", "width": 500, "height": 350}})
    assert base64.b64decode(pdf["image"]["pdf_base64"]).startswith(b"%PDF-")


def test_the_methods_are_in_describe_and_in_the_mcp_domains(cad, mcp):
    names = {m["name"] for m in cad.result("describe")["methods"]}
    assert {"import.update", "import.apply", "dxf.compare"} <= names
    d = cad.result("describe")
    assert d["io"]["update"]["examples"] and d["review"]["dxf_compare"]["examples"]
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    assert {"import.update", "import.apply"} <= set(tools["io"]["inputSchema"]["properties"]["method"]["enum"])
    assert "dxf.compare" in tools["review"]["inputSchema"]["properties"]["method"]["enum"]
