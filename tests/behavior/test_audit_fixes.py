"""Scenario: files, converters and SVG output behave the same in every host."""

import json

from helpers import McpClient, rect_cmd, run_cli

from conftest import ALLOW


def test_serve_saves_and_opens_cdk_and_cdk_json(cad, make_serve, tmp_path):
    cad.execute(rect_cmd(0, 0, 1, 1))
    for name in ("plan.cdk", "plan.cdk.json"):
        f = tmp_path / name
        assert cad.result("file.save", {"path": str(f)}) == {"ok": True}
        assert json.loads(f.read_text())["format_version"] == 2
        other = make_serve()
        assert other.result("file.open", {"path": str(f)}) == {"ok": True}
        assert other.result("document.summary")["object_count"] == 1
    # A second save keeps the first file as .bak.
    cad.execute(rect_cmd(2, 2, 3, 3))
    cad.result("file.save", {"path": str(tmp_path / "plan.cdk")})
    assert json.loads((tmp_path / "plan.cdk.bak").read_text())["objects"][0]["id"]
    assert cad.error("file.save", {"path": str(tmp_path / "x.txt")})["code"] == 4


def test_mcp_saves_and_opens_cdk(tmp_path):
    f = tmp_path / "m.cdk"
    mcp = McpClient(["mcp", *ALLOW])
    mcp.initialize()
    try:
        mcp.tool_json("execute", {"command": rect_cmd(0, 0, 1, 1)})
        assert mcp.tool_json("file_save", {"path": str(f)}) == {"ok": True}
        assert f.exists()
        assert mcp.tool_json("file_open", {"path": str(f)}) == {"ok": True}
    finally:
        mcp.close()


def test_a_newer_file_gets_a_clear_message(cad, tmp_path):
    f = tmp_path / "future.cdk"
    f.write_text(json.dumps({"format_version": 99, "layers": "???"}))
    err = cad.error("file.open", {"path": str(f)})
    assert "file was made by a newer Cadektor (format 99)" in err["message"]
    assert "not a Cadektor file" not in err["message"]
    assert err["data"]["kind"] == "newer_format"


def test_future_shapes_and_fields_survive_open_and_save(cad, make_serve, tmp_path):
    cad.execute(rect_cmd(0, 0, 1, 1))
    doc = cad.doc()
    doc["objects"][0]["shape"] = {"kind": "spiral", "turns": 3}
    doc["objects"][0]["future"] = {"a": 1}
    doc["layers"][0]["future_layer"] = True
    doc["new_top"] = [1, 2]
    src = tmp_path / "future.cdk"
    src.write_text(json.dumps(doc))
    cad.result("file.open", {"path": str(src)})
    summary = cad.result("document.summary")
    assert summary["objects_by_kind"] == {"unknown": 1}
    assert summary["unknown_shapes"][0]["kind"] == "spiral"
    out = tmp_path / "again.cdk"
    cad.result("file.save", {"path": str(out)})
    assert json.loads(out.read_text()) == doc
    oid = summary["unknown_shapes"][0]["id"]
    assert cad.error("execute", {"command": {"op": "move_objects", "ids": [oid], "delta": [1, 1]}})["code"] == 3
    cad.execute({"op": "delete_objects", "ids": [oid]})
    assert cad.result("document.summary")["object_count"] == 0


def test_dwg_needs_allow_converters(make_serve, tmp_path):
    dwg = tmp_path / "a.dwg"
    dwg.write_bytes(b"x")
    off = make_serve()
    err = off.error("import.file", {"path": str(dwg)})
    assert err["code"] == 4 and err["data"]["kind"] == "policy" and "--allow-converters" in err["message"]
    assert off.error("export.file", {"path": str(tmp_path / "b.dwg"), "format": "dwg"})["code"] == 4
    on = make_serve("--allow-converters")
    err = on.error("import.file", {"path": str(dwg)})
    assert "--allow-converters" not in err["message"]


def test_cli_render_is_the_same_svg_as_the_api(cad, tmp_path):
    cad.execute(rect_cmd(0, 0, 4000, 3000))
    f = tmp_path / "r.cdk"
    cad.result("file.save", {"path": str(f)})
    out = tmp_path / "r.svg"
    p = run_cli("render", str(f), "-o", str(out), "--width", "700")
    assert p.returncode == 0, p.stderr
    assert out.read_text() == cad.result("render.svg", {"width": 700})["svg"]


def test_exports_keep_a_bak(cad, tmp_path):
    cad.execute({"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [3000, 0], [3000, 1200], [0, 1200]], "material": "Granite"}})
    out = tmp_path / "m.glb"
    cad.result("export.file", {"path": str(out)})
    first = out.read_bytes()
    cad.result("export.file", {"path": str(out), "overwrite": True})
    assert (tmp_path / "m.glb.bak").read_bytes() == first
