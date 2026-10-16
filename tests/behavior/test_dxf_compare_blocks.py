"""Scenario: two revisions of a survey are compared. A tree block did not change, but the second file
was saved again and numbers the block definition differently. The contents of the block (on layer 0)
are not changes, and the revision clouds are outlines that leave the changes visible."""

import base64

import pytest

from test_dxf_update import CLOUD, decode_png

ezdxf = pytest.importorskip("ezdxf", reason="run with: uv run --with pytest --with ezdxf pytest tests/behavior")


def survey(path, rev):
    doc = ezdxf.new("R2018", setup=True)
    doc.header["$INSUNITS"] = 4
    doc.layers.add("Trees")
    doc.layers.add("Fence")
    if rev == 2:
        # More layers before the block: the program that saves the file numbers the block record
        # of the unchanged tree block differently.
        for i in range(5):
            doc.layers.add(f"New{i}")
    tree = doc.blocks.new("TREE")
    tree.add_circle((0, 0), 400)
    tree.add_line((-400, 0), (400, 0))
    tree.add_line((0, -400), (0, 400))
    msp = doc.modelspace()
    for i in range(4):
        msp.add_blockref("TREE", (3000 + i * 4000, 3000), dxfattribs={"layer": "Trees"})
    east = 20000 if rev == 1 else 20040
    msp.add_line((east, 0), (east, 12000), dxfattribs={"layer": "Fence"})
    msp.add_line((0, 0), (0, 12000), dxfattribs={"layer": "Fence"})
    doc.saveas(str(path))
    return doc.blocks.get("TREE").block_record.dxf.handle


def test_an_unchanged_block_is_not_a_change_even_when_its_handles_differ(cad, tmp_path):
    v1, v2 = tmp_path / "rev1.dxf", tmp_path / "rev2.dxf"
    h1, h2 = survey(v1, 1), survey(v2, 2)
    assert h1 != h2, "the test needs a renumbered block definition"
    r = cad.result("dxf.compare", {"base": str(v1), "new": str(v2)})
    assert r["counts"]["added"] == 0 and r["counts"]["removed"] == 0, r["changes"]
    assert r["counts"]["changed"] == 1
    assert [c["entity"] for c in r["changes"]] == ["LINE"]
    assert not [c for c in r["changes"] if c["layer"] == "0"]


def test_a_block_that_changed_is_still_reported(cad, tmp_path):
    v1, v2 = tmp_path / "rev1.dxf", tmp_path / "rev2.dxf"
    survey(v1, 1)
    survey(v2, 2)
    doc = ezdxf.readfile(str(v2))
    next(iter(doc.blocks.get("TREE").query("CIRCLE"))).dxf.radius = 700
    doc.saveas(str(v2))
    r = cad.result("dxf.compare", {"base": str(v1), "new": str(v2)})
    circles = [c for c in r["changes"] if c["entity"] == "CIRCLE" and c["layer"] == "0"]
    assert circles, r["changes"]
    # The lines of the block did not change.
    assert not [c for c in r["changes"] if c["entity"] == "LINE" and c["layer"] == "0"]


def test_import_update_agrees_with_compare_on_a_resaved_block(cad, tmp_path):
    v1, v2 = tmp_path / "rev1.dxf", tmp_path / "rev2.dxf"
    survey(v1, 1)
    survey(v2, 2)
    cad.result("import.file", {"path": str(v1)})
    r = cad.result("import.update", {"path": str(v2)})
    assert r["counts"]["added"] == 0 and r["counts"]["removed"] == 0, r["changes"]
    assert [c["entity"] for c in r["changes"]] == ["LINE"]
    assert not [c for c in r["changes"] if c["layer"] == "0"]


def test_revision_clouds_are_outlines(cad, tmp_path):
    v1, v2 = tmp_path / "rev1.dxf", tmp_path / "rev2.dxf"
    survey(v1, 1)
    survey(v2, 2)
    r = cad.result(
        "dxf.compare",
        {"base": str(v1), "new": str(v2), "render": {"format": "png", "revision_clouds": True, "width": 600, "height": 400}},
    )
    assert r["image"]["clouds"]["count"] == 1
    w, h, bpp, rows = decode_png(base64.b64decode(r["image"]["png_base64"]))
    near = 0
    for row in rows:
        for x in range(0, len(row), bpp):
            if sum((row[x + k] - CLOUD[k]) ** 2 for k in range(3)) ** 0.5 < 60:
                near += 1
    # The outline of a cloud is a thin line. A filled cloud would cover a large part of the picture.
    assert 0 < near < 0.03 * w * h, f"{near} of {w * h} pixels have the color of the cloud"
