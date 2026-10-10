"""Scenario: an AI agent connects to `kadakas mcp` and draws through the MCP tools."""

import json

import pytest

from conftest import ALLOW
from helpers import McpClient, plant_cmd, rect_cmd


def test_initialize_reports_server_info_and_tools_capability(mcp_raw):
    resp = mcp_raw.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}})
    result = resp["result"]
    assert result["serverInfo"]["name"] == "kadakas"
    assert "tools" in result["capabilities"]
    assert "describe" in result["instructions"]


@pytest.mark.parametrize("version", ["2025-06-18", "2025-03-26"])
def test_initialize_echoes_a_supported_protocol_version(mcp_raw, version):
    resp = mcp_raw.request("initialize", {"protocolVersion": version, "capabilities": {}})
    assert resp["result"]["protocolVersion"] == version


@pytest.mark.parametrize("version", ["2024-11-05", "2099-01-01", "nonsense"])
def test_initialize_answers_with_the_latest_supported_version_for_unknown_ones(mcp_raw, version):
    resp = mcp_raw.request("initialize", {"protocolVersion": version, "capabilities": {}})
    assert resp["result"]["protocolVersion"] == "2025-06-18"


def test_initialize_without_a_version_answers_with_the_latest(mcp_raw):
    resp = mcp_raw.request("initialize", {"capabilities": {}})
    assert resp["result"]["protocolVersion"] == "2025-06-18"


def listed(mcp):
    return {tool["name"]: tool for tool in mcp.request("tools/list")["result"]["tools"]}


def test_tools_list_has_about_twenty_domain_tools(mcp):
    tools = listed(mcp)
    assert 15 <= len(tools) <= 25, sorted(tools)
    assert {"describe", "document", "objects", "plants", "annotation", "io", "gis", "scripting"} <= set(tools)
    assert all("." not in name for name in tools)


def test_every_api_method_is_in_exactly_one_domain_tool(mcp):
    tools = listed(mcp)
    seen = []
    for tool in tools.values():
        seen += tool["inputSchema"]["properties"]["method"]["enum"]
    assert len(seen) == len(set(seen))
    api = {m["name"] for m in mcp.method_json("describe", "describe")["methods"]}
    # The methods that need a message stream are not tools: a tool call returns one result.
    stream = {"events.subscribe", "events.unsubscribe"}
    assert stream <= api
    assert (api - stream) | {"file.save", "file.open"} == set(seen)
    for tool in tools.values():
        for method in tool["inputSchema"]["properties"]["method"]["enum"]:
            assert f"- {method}:" in tool["description"], (tool["name"], method)


def test_every_tool_takes_method_and_params_and_has_annotations(mcp):
    for name, tool in listed(mcp).items():
        assert tool["description"], name
        schema = tool["inputSchema"]
        assert schema["type"] == "object" and schema["required"] == ["method"], name
        assert schema["properties"]["params"]["type"] == "object", name
        assert isinstance(tool["annotations"]["readOnlyHint"], bool), name
    tools = listed(mcp)
    for name in ("describe", "objects", "reports"):
        assert tools[name]["annotations"]["readOnlyHint"] is True
    assert tools["plants"]["annotations"] == {"readOnlyHint": False, "destructiveHint": True}


def test_flat_listing_keeps_one_tool_per_method(mcp_flat):
    names = {tool["name"] for tool in mcp_flat.request("tools/list")["result"]["tools"]}
    assert {"describe", "execute", "file_save", "file_open"} <= names
    for api_name in ["document.summary", "objects.list", "selection.set", "render.svg", "report.plants"]:
        assert api_name.replace(".", "_") in names
    assert all("." not in name for name in names)
    assert len(names) > 250
    for tool in mcp_flat.request("tools/list")["result"]["tools"]:
        assert tool["inputSchema"]["type"] == "object", tool["name"]


def test_execute_method_draws_and_report_method_reads_back(mcp):
    created = mcp.method_json("document", "execute", {"command": plant_cmd(0, 0, species="Oak")})["created"]
    assert len(created) == 1
    rows = mcp.method_json("reports", "report.plants")["rows"]
    assert rows == [{"species": "Oak", "count": 1}]


def test_flat_tools_draw_and_read_back(mcp_flat):
    created = mcp_flat.tool_json("execute", {"command": plant_cmd(0, 0, species="Oak")})["created"]
    assert len(created) == 1
    assert mcp_flat.tool_json("report_plants")["rows"] == [{"species": "Oak", "count": 1}]
    assert mcp_flat.tool("objects_get", {})["isError"] is True


def test_describe_tool_returns_units_and_command_examples(mcp):
    description = mcp.method_json("describe", "describe")
    assert description["command_examples"]
    assert "millimeters" in description["units"]


def test_tool_error_comes_back_as_is_error_true(mcp):
    result = mcp.method("document", "execute", {"command": {"op": "add_object", "shape": {
        "kind": "circle", "center": [0, 0], "radius": -1}}})
    assert result["isError"] is True
    assert "radius must be positive" in result["content"][0]["text"]


def test_tool_with_bad_params_comes_back_as_is_error_true(mcp):
    result = mcp.method("objects", "objects.get", {})
    assert result["isError"] is True


def test_a_call_without_a_method_or_with_a_method_of_another_domain_is_a_tool_error(mcp):
    result = mcp.tool("objects", {})
    assert result["isError"] is True and "needs params.method" in result["content"][0]["text"]
    result = mcp.method("objects", "plants.place", {})
    assert result["isError"] is True
    text = result["content"][0]["text"]
    assert "has no method 'plants.place'" in text and "objects.list" in text


def test_unknown_tool_is_a_jsonrpc_error(mcp):
    resp = mcp.request("tools/call", {"name": "does_not_exist", "arguments": {}})
    assert "result" not in resp
    assert resp["error"]["code"] == -32602
    assert "does_not_exist" in resp["error"]["message"]
    # The flat names are not tools in the domain listing.
    assert mcp.request("tools/call", {"name": "execute", "arguments": {}})["error"]["code"] == -32602


def test_ping_gets_an_empty_result(mcp):
    assert mcp.request("ping")["result"] == {}


def test_unknown_method_is_method_not_found(mcp):
    assert mcp.request("resources/list")["error"]["code"] == -32601


def test_notifications_get_no_response(mcp):
    mcp.notify("notifications/cancelled", {"requestId": 1})
    mcp.expect_silence()
    assert mcp.request("ping")["result"] == {}


def test_file_save_then_file_open_round_trip_through_tools(mcp, tmp_path):
    path = tmp_path / "agent.json"
    mcp.method_json("document", "execute", {"command": rect_cmd(0, 0, 500, 500)})
    assert mcp.method_json("document", "file.save", {"path": str(path)}) == {"ok": True}
    expected = mcp.method_json("document", "document.get")

    other = McpClient(["mcp", *ALLOW])
    try:
        other.initialize()
        assert other.method_json("document", "file.open", {"path": str(path)}) == {"ok": True}
        assert other.method_json("document", "document.get") == expected
    finally:
        other.close()


def test_file_save_without_a_path_is_an_error(mcp):
    result = mcp.method("document", "file.save", {})
    assert result["isError"] is True


def test_unparseable_line_gets_a_parse_error_response(mcp):
    mcp.send_raw("{not json")
    line = mcp.read_line(timeout=2)
    assert line is not None, "no response to an unparseable line"
    resp = json.loads(line)
    assert resp["error"]["code"] == -32700
    assert resp["id"] is None
