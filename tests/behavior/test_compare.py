"""Scenario: a designer saves a version of a garden plan, changes it, and an agent that reads images
asks for the compare picture: added objects green, changed objects orange, removed objects as red
ghosts, in `render.png`, `render.svg` and `sheet.render` (REV-03)."""

import base64

from helpers import McpClient, rect_cmd
from test_render_png import decode_png

BED_FILL = [120, 120, 120, 255]


def square(x, y, **extra):
    return rect_cmd(x, y, x + 3000, y + 3000, attrs={"fill": BED_FILL}, **extra)


def garden(cad):
    """A removed bed, a bed that moves, one that stays, and (after the version) a new bed."""
    removed = cad.execute(square(0, 0))[0]
    moved = cad.execute(square(5000, 0))[0]
    same = cad.execute(square(15000, 0))[0]
    cad.result("version.create", {"name": "Concept", "now_ms": 1000})
    cad.execute({"op": "delete_objects", "ids": [removed]})
    cad.execute({"op": "move_objects", "ids": [moved], "delta": [0, 8000]})
    added = cad.execute(square(10000, 0))[0]
    return {"removed": removed, "moved": moved, "same": same, "added": added}


def mean_color(rows, pick):
    """The mean color and the mean column of the pixels that `pick` selects."""
    total, cols, n = [0, 0, 0], 0, 0
    for row in rows:
        for x, (r, g, b, _) in enumerate(row):
            if pick(r, g, b):
                total = [total[0] + r, total[1] + g, total[2] + b]
                cols += x
                n += 1
    assert n, "no such pixels"
    return [t / n for t in total], cols / n, n


def test_the_compare_png_tints_what_changed(cad, tmp_path):
    garden(cad)
    r = cad.result(
        "render.png",
        {"view": "top", "projection": "ortho", "width": 600, "height": 200,
         "compare": {"from": 1, "to": "current"}},
    )
    (tmp_path / "compare.png").write_bytes(base64.b64decode(r["png_base64"]))
    assert (r["width"], r["height"]) == (600, 200)
    info = r["compare"]
    assert info["counts"] == {"added": 1, "removed": 1, "changed": 1}
    assert info["summary"]["ghosts_drawn"] == 1 and info["summary"]["exact"] is True
    assert info["notes"] == []
    assert [(l["status"], l["symbol"], l["pattern"]) for l in info["legend"]] == [
        ("added", "+", "solid"), ("removed", "-", "dashed"), ("changed", "~", "dotted")]
    _, _, rows = decode_png(r["png_base64"])
    green, green_x, _ = mean_color(rows, lambda r, g, b: g > r + 60 and g > b + 40)
    orange, orange_x, _ = mean_color(rows, lambda r, g, b: r > 150 and r > g + 50 and g > b + 50)
    ghost, ghost_x, _ = mean_color(
        rows, lambda r, g, b: r > g + 30 and r > b + 30 and abs(g - b) < 25 and g > 90)
    assert green[1] > green[0] + 80
    assert orange[0] > orange[1] > orange[2]
    assert ghost[0] > ghost[1], "the ghost is red, mixed with the background"
    # From left to right: the removed bed, the moved bed, the new bed.
    assert ghost_x < orange_x < green_x


def test_the_compare_svg_has_groups_marks_and_a_legend(cad):
    ids = garden(cad)
    svg = cad.result("render.svg", {"compare": {"from": 1, "to": "current"}})["svg"]
    for status in ("added", "changed"):
        assert f'<g data-compare="{status}"' in svg
    assert 'data-ghost="removed"' in svg and ids["removed"] in svg
    assert 'data-compare="unchanged"' in svg
    assert 'data-mark="added">+<' in svg and 'data-mark="removed">-<' in svg
    assert ">added 1</text>" in svg and ">removed 1</text>" in svg
    hidden = cad.result("render.svg", {"compare": {"from": 1, "to": "current", "hide_unchanged": True}})["svg"]
    assert ids["same"] not in hidden and ids["added"] in hidden
    blind = cad.result("render.svg", {"compare": {"from": 1, "to": "current", "palette": "color_blind"}})["svg"]
    assert "rgba(0,114,178,1.000)" in blind and "rgba(0,128,48,1.000)" not in blind


def test_side_by_side_and_sheets(cad):
    garden(cad)
    side = cad.result("render.svg", {"compare": {"from": 1, "to": "current", "mode": "side_by_side"}})
    assert side["svg"].count("<svg ") == 3
    assert "data-compare=" not in side["svg"].replace('data-compare-mode="side_by_side"', "")
    png = cad.result("render.png", {"view": "top", "width": 404, "height": 100,
                                    "compare": {"from": 1, "to": "current", "mode": "side_by_side"}})
    assert (png["width"], png["height"]) == (404, 100)
    cad.result("sheet.create", {"name": "S"})
    cad.result("viewport.create", {"sheet": "S"})
    cad.result("version.create", {"name": "With sheet", "now_ms": 2000})
    cad.execute(rect_cmd(1000, 1000, 2000, 2000))
    sheet = cad.result("sheet.render", {"sheet": "S", "compare": {"from": 2, "to": "current"}})
    assert "With sheet" in sheet["svg"] and "current drawing" in sheet["svg"]
    assert sheet["compare"]["mode"] == "side_by_side"


def test_errors_are_clear(cad):
    garden(cad)
    for bad, text in [
        ({"from": 9, "to": "current"}, "version"),
        ({"from": 1}, "compare.to"),
        ({"from": 1, "to": "current", "mode": "both"}, "side_by_side"),
        ({"from": 1, "to": "current", "colour": "red"}, "unknown compare option"),
    ]:
        err = cad.error("render.png", {"compare": bad})
        assert text in err["message"], err


def test_the_compare_option_is_in_describe_and_mcp(cad, mcp: McpClient):
    d = cad.result("describe")
    docs = {m["name"]: m["doc"] for m in d["methods"]}
    for method in ("render.png", "render.svg", "sheet.render"):
        assert "compare" in docs[method]
    assert "side_by_side" in d["review"]["compare"]["option"]
    assert any(e["method"] == "render.png" for e in d["review"]["examples"])
    mcp.method_json("document", "execute", {"command": square(0, 0)})
    mcp.method_json("review", "version.create", {"name": "A"})
    mcp.method_json("document", "execute", {"command": square(9000, 0)})
    result = mcp.method("render_view", "render.svg", {"compare": {"from": 1, "to": "current"}})
    assert result["isError"] is False
    assert "data-compare=" in result["content"][0]["text"]
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    assert "compare" in tools["review"]["description"]
