"""Scenario: an agent asks for a screenshot of the live viewport (API-14).

Only the app has a window. The headless server explains this. The app side is tested in the app
crate (request parsing, cutting and scaling of the picture)."""


def test_the_screenshot_method_is_described_and_known(cad):
    desc = cad.result("describe")
    entry = next(m for m in desc["methods"] if m["name"] == "view.screenshot")
    assert "region" in entry["doc"] and "max_width" in entry["doc"] and "png_base64" in entry["doc"]


def test_the_headless_server_says_why_there_is_no_screenshot(cad):
    err = cad.error("view.screenshot", {"region": "window"})
    assert err["code"] == 1
    assert err["data"]["kind"] == "error"
    assert "live window" in err["message"] and "render.png" in err["message"]
    # The session stays healthy.
    assert cad.result("document.summary")["object_count"] == 0


def test_the_screenshot_method_is_in_the_render_view_tool_of_mcp(mcp):
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    assert "view.screenshot" in tools["render_view"]["description"]


def test_the_editing_methods_have_a_tool_of_their_own_in_mcp(mcp):
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    text = tools["edit_2d"]["description"]
    for name in ["edit.fillet", "edit.trim", "surface.compose", "snap.query"]:
        assert f"- {name}:" in text
    # A call through the tool runs the method.
    created = mcp.method_json("document", "execute", {"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}})["created"]
    out = mcp.method_json("edit_2d", "edit.mirror", {"ids": created, "a": [0, 0], "b": [0, 1], "copy": True})
    assert len(out["created"]) == 1
    rows = mcp.method_json("objects", "objects.query", {"criteria": 'Kind = "circle"'})
    assert rows["count"] == 2
