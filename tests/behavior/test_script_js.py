"""Scenario: an agent writes JavaScript that runs inside the session (SCR-08), over JSON-RPC and
MCP."""

CIRCLE = {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 100}}


def test_describe_has_the_javascript_section_with_examples(cad):
    d = cad.result("describe")
    js = d["scripting"]["javascript"]
    assert "Programs only coordinate" in js["rule"]
    assert js["examples"] and js["examples"][0]["method"] == "script.eval"
    assert "documents_hold_no_code" in js
    names = [m["name"] for m in d["methods"]]
    assert {"script.eval", "script.js_info"} <= set(names)
    assert "script_resource.create" not in names
    assert "generators" in d["unit_patterns"]


def test_a_script_calls_the_api_and_returns_a_value(cad):
    r = cad.result("script.eval", {
        "code": "const s = kadakas.call('document.summary', {}); return {layers: s.layers.length, n: args.n};",
        "args": {"n": 3}, "mode": "read"})
    assert r["result"] == {"layers": 1, "n": 3}
    assert r["calls"] == 1 and r["undo_steps"] == 0


def test_one_undo_step_and_a_rollback_on_error(cad):
    code = "for (let i = 0; i < 3; i++) kadakas.call('execute', {command: args.c});"
    r = cad.result("script.eval", {"code": code, "args": {"c": CIRCLE}})
    assert r["undo_steps"] == 1 and len(cad.result("objects.list")) == 3
    cad.result("undo")
    assert cad.result("objects.list") == []
    err = cad.error("script.eval", {"code": code + "throw new Error('no');", "args": {"c": CIRCLE}})
    assert "no" in err["message"]
    assert cad.result("objects.list") == []


def test_read_mode_allows_read_only_methods_and_refuses_the_rest_up_front(cad):
    err = cad.error("script.eval", {"code": "kadakas.call('execute', {command: args.c});",
                                    "args": {"c": CIRCLE}, "mode": "read"})
    assert "read-only" in err["message"]
    assert cad.result("objects.list") == []


def test_unknown_parameters_are_refused(cad):
    err = cad.error("script.eval", {"code": "1", "colour": "red"})
    assert err["code"] == -32602
    err = cad.error("script.eval", {"script": "x"})
    assert err["code"] == -32602


def test_a_script_runs_the_same_every_time(cad):
    code = "return [Math.random(), Date.now()];"
    a = cad.result("script.eval", {"code": code, "seed": 9})["result"]
    b = cad.result("script.eval", {"code": code, "seed": 9})["result"]
    assert a == b and a[1] == 0


def test_a_generator_pattern_stores_its_result_and_needs_no_code(cad):
    pattern = {"kind": "generator", "types": ["a"], "gap": 0,
               "rects": {"data": [100, 50, 200, 100, 0, 300, 50, 200, 100, 0]}}
    cad.result("script.eval", {"code": "return 1;"})  # the engine is not needed for the layout below
    layout = cad.result("unit_pattern.layout", {
        "region": {"outline": [[0, 0], [400, 0], [400, 100], [0, 100]]},
        "pattern": pattern, "origin": [0, 0]})
    assert layout["units_total"] == 2
    err = cad.error("unit_pattern.layout", {
        "region": {"outline": [[0, 0], [10, 0], [10, 10], [0, 10]]},
        "pattern": {"kind": "generator", "gap": 0}, "origin": [0, 0]})
    assert "no stored result" in err["message"]


def test_the_mcp_scripting_tool_lists_the_script_methods(mcp):
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    assert "script.eval" in tools["scripting"]["description"]
    assert tools["scripting"]["annotations"].get("openWorldHint") is True
    out = mcp.method_json("scripting", "script.eval", {"code": "return 1 + 1;", "mode": "read"})
    assert out["result"] == 2
