"""Scenario: an AI agent connects to `cadektor mcp` and draws through the MCP tools."""

import json

import pytest

from conftest import ALLOW
from helpers import McpClient, plant_cmd, rect_cmd


def test_initialize_reports_server_info_and_tools_capability(mcp_raw):
    resp = mcp_raw.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}})
    result = resp["result"]
    assert result["serverInfo"]["name"] == "cadektor"
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


def test_tools_list_includes_describe_execute_and_file_save(mcp):
    names = {tool["name"] for tool in mcp.request("tools/list")["result"]["tools"]}
    assert {"describe", "execute", "file_save", "file_open"} <= names


def test_tools_list_maps_every_api_method_to_a_tool_name(mcp):
    names = {tool["name"] for tool in mcp.request("tools/list")["result"]["tools"]}
    for api_name in ["document.summary", "objects.list", "selection.set", "render.svg", "report.plants"]:
        assert api_name.replace(".", "_") in names
    assert all("." not in name for name in names)


def test_every_tool_declares_an_object_input_schema(mcp):
    for tool in mcp.request("tools/list")["result"]["tools"]:
        assert tool["description"], tool["name"]
        assert tool["inputSchema"]["type"] == "object", tool["name"]


def test_execute_tool_draws_and_report_tool_reads_back(mcp):
    created = mcp.tool_json("execute", {"command": plant_cmd(0, 0, species="Oak")})["created"]
    assert len(created) == 1
    rows = mcp.tool_json("report_plants")["rows"]
    assert rows == [{"species": "Oak", "count": 1}]


def test_describe_tool_returns_units_and_command_examples(mcp):
    description = mcp.tool_json("describe")
    assert description["command_examples"]
    assert "millimeters" in description["units"]


def test_tool_error_comes_back_as_is_error_true(mcp):
    result = mcp.tool("execute", {"command": {"op": "add_object", "shape": {
        "kind": "circle", "center": [0, 0], "radius": -1}}})
    assert result["isError"] is True
    assert "radius must be positive" in result["content"][0]["text"]


def test_tool_with_bad_params_comes_back_as_is_error_true(mcp):
    result = mcp.tool("objects_get", {})
    assert result["isError"] is True


def test_unknown_tool_is_a_jsonrpc_error(mcp):
    resp = mcp.request("tools/call", {"name": "does_not_exist", "arguments": {}})
    assert "result" not in resp
    assert resp["error"]["code"] == -32602
    assert "does_not_exist" in resp["error"]["message"]


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
    mcp.tool_json("execute", {"command": rect_cmd(0, 0, 500, 500)})
    assert mcp.tool_json("file_save", {"path": str(path)}) == {"ok": True}
    expected = mcp.tool_json("document_get")

    other = McpClient(["mcp", *ALLOW])
    try:
        other.initialize()
        assert other.tool_json("file_open", {"path": str(path)}) == {"ok": True}
        assert other.tool_json("document_get") == expected
    finally:
        other.close()


def test_file_save_without_a_path_is_an_error(mcp):
    result = mcp.tool("file_save", {})
    assert result["isError"] is True


def test_unparseable_line_gets_a_parse_error_response(mcp):
    mcp.send_raw("{not json")
    line = mcp.read_line(timeout=2)
    assert line is not None, "no response to an unparseable line"
    resp = json.loads(line)
    assert resp["error"]["code"] == -32700
    assert resp["id"] is None
