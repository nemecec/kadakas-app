"""Scenario: an agent finds a missing feature or a failing method and files a report (API-47, API-48).

The child processes have a private home (helpers.isolated_env), so the outbox is in a temp folder.
Nothing here uses the network: no test turns sending on."""

import json

import pytest

from conftest import ALLOW
from helpers import JsonRpc, McpClient, plant_cmd, rect_cmd, run_cli

KINDS = ["bug", "feature_request", "question"]


@pytest.fixture
def outbox(tmp_path):
    """Every test has its own outbox folder: reports are kept between runs, and merge by key."""
    return {"KADAKAS_FEEDBACK_DIR": str(tmp_path / "outbox")}


@pytest.fixture
def cad(outbox):
    client = JsonRpc(["serve", *ALLOW], env=outbox)
    yield client
    client.close()


@pytest.fixture
def rpc(cad):
    return cad.rpc


def test_describe_has_a_feedback_section_with_examples(rpc):
    d = rpc("describe")["result"]
    fb = d["feedback"]
    assert "feature request" in fb["purpose"] and "bug report" in fb["purpose"]
    assert len(fb["examples"]) >= 4
    assert "needs_review" in fb["consent"]
    assert {"feedback.submit", "feedback.list", "feedback.get", "feedback.withdraw"} <= {
        m["name"] for m in d["methods"]
    }
    assert "internal" in {c["kind"] for c in d["errors"]["codes"]}


def test_an_unknown_method_carries_a_ready_draft_that_can_be_submitted_as_is(cad):
    err = cad.error("plants.teleport", {})
    assert err["code"] == -32601
    hint = err["data"]["feedback"]
    assert len(hint["hint"]) < 200 and "describe" in hint["hint"]
    draft = hint["draft"]
    assert set(draft) == {"kind", "title", "method"}
    assert draft["kind"] == "feature_request" and draft["method"] == "plants.teleport"
    report = cad.result("feedback.submit", draft)
    assert report["merged"] is False
    # The same draft again merges into a counter.
    again = cad.result("feedback.submit", draft)
    assert (again["id"], again["merged"], again["count"]) == (report["id"], True, 2)
    assert cad.result("feedback.list")["reports"][0]["count"] == 2


def test_an_unsupported_option_has_a_hint_but_a_plain_mistake_has_none(cad):
    err = cad.error("massing.add", {"footprint": [[0, 0], [1000, 0], [1000, 1000]], "bogus": 1})
    assert err["data"]["feedback"]["draft"]["method"] == "massing.add"
    assert "bogus" in err["data"]["feedback"]["draft"]["title"]
    assert "feedback" not in cad.error("objects.get", {})["data"]


def test_a_bug_report_attaches_context_and_waits_for_a_human(cad):
    created = cad.execute(rect_cmd(0, 0, 4000, 3000))
    cad.execute(plant_cmd(1000, 1000))
    assert "error" in cad.rpc("edit.fillet", {"id": created[0], "radius": -5})
    r = cad.result("feedback.submit", {"kind": "bug", "title": "Fillet fails with a negative radius"})
    assert r["status"] == "needs_review"
    assert r["attachments"] == ["excerpt", "render", "calls", "log"]
    got = cad.result("feedback.get", {"id": r["id"]})
    # The method and the error come from the failed call.
    assert got["method"] == "edit.fillet" and "code" in got["error"]
    assert got["origin"] == "agent" and got["context"]["version"]
    assert got["attachments"][0]["kind"] == "excerpt" and got["attachments"][0]["objects"] == 1
    listing = cad.result("feedback.list")
    assert listing["needs_review"] >= 1 and listing["sending"] == {"enabled": False, "asked": False}


def test_an_agent_cannot_approve_or_send_attachments(cad):
    cad.execute(rect_cmd(0, 0, 100, 100))
    r = cad.result("feedback.submit", {"kind": "bug", "title": "Waiting for a person"})
    for method in ("feedback.approve", "feedback.send", "feedback.review"):
        assert cad.error(method, {"id": r["id"]})["code"] == -32601
    # Extra fields are refused and change nothing.
    for method, params in (
        ("feedback.get", {"id": r["id"], "status": "approved", "approve": True}),
        ("feedback.submit", {"kind": "bug", "title": "Waiting for a person", "status": "approved"}),
    ):
        assert "unknown parameter" in cad.error(method, params)["message"]
    assert cad.result("feedback.get", {"id": r["id"]})["status"] == "needs_review"


def test_attach_can_remove_each_part(cad):
    cad.execute(rect_cmd(0, 0, 100, 100))
    r = cad.result("feedback.submit", {"kind": "bug", "title": "No picture", "attach": {"screenshot": False, "log": False}})
    assert r["attachments"] == ["excerpt", "calls"]
    r = cad.result("feedback.submit", {"kind": "bug", "title": "Nothing", "attach": False})
    assert r["attachments"] == [] and r["status"] == "pending"
    r = cad.result("feedback.submit", {"kind": "feature_request", "title": "Want a thing"})
    assert r["attachments"] == ["render", "calls"]


def test_texts_and_the_call_log_are_scrubbed(cad):
    secret = "Qx7Lm2Pz90Ab"
    cad.rpc("file.open", {"path": f"/home/{secret}/plans/garden.kad", "token": secret})
    r = cad.result(
        "feedback.submit",
        {"kind": "question", "title": f"Mail {secret}@example.com", "body": f"token={secret} at /Users/{secret}/a.kad"},
    )
    got = cad.result("feedback.get", {"id": r["id"]})
    assert secret not in json.dumps(got)
    assert "<email>" in got["title"] and "<path>/a.kad" in got["body"]


def test_bounds_and_withdraw(cad):
    r = cad.result("feedback.submit", {"kind": "question", "title": "t" * 500, "body": "b" * 9000, "attach": False})
    got = cad.result("feedback.get", {"id": r["id"]})
    assert len(got["title"]) <= 121 and len(got["body"]) <= 4001
    assert cad.error("feedback.submit", {"kind": "bug", "title": "  "})["code"] == -32602
    assert cad.error("feedback.submit", {"kind": "idea", "title": "x"})["code"] == -32602
    assert cad.result("feedback.withdraw", {"id": r["id"]})["status"] == "withdrawn"
    assert cad.error("feedback.get", {"id": "fb-99"})["code"] == 2


def test_the_session_rate_limit(cad):
    for i in range(30):
        cad.result("feedback.submit", {"kind": "question", "title": f"q{i}", "attach": False})
    assert cad.error("feedback.submit", {"kind": "question", "title": "one more", "attach": False})["code"] == 6


@pytest.fixture(params=["domain", "flat"])
def either(request, outbox):
    args = ["mcp", *ALLOW] + (["--mcp-flat"] if request.param == "flat" else [])
    client = McpClient(args, env=outbox)
    yield client
    client.close()


def test_mcp_instructions_and_the_feedback_tool_in_both_modes(either):
    init = either.request(
        "initialize",
        {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "pytest-agent", "version": "1.2.3"}},
    )["result"]
    assert "feedback tool" in init["instructions"]
    assert "feature request" in init["instructions"] and "bug report" in init["instructions"]
    either.notify("notifications/initialized")
    tools = either.request("tools/list")["result"]["tools"]
    tool = next(t for t in tools if t["name"] == "feedback")
    assert tools[1]["name"] == "feedback"
    assert "must" in tool["description"] and "feature_request" in tool["description"]
    props = tool["inputSchema"]["properties"]
    assert props["kind"]["enum"] == KINDS
    assert {"title", "body", "wanted", "tried", "method", "error", "severity", "attach", "action", "id"} <= set(props)

    r = either.tool_json("feedback", {"kind": "feature_request", "title": "Mirror along a line", "attach": False})
    got = either.tool_json("feedback", {"action": "get", "id": r["id"]})
    assert got["context"]["client_name"] == "pytest-agent" and got["context"]["client_version"] == "1.2.3"
    assert either.tool("feedback", {"action": "approve"})["isError"] is True


def test_mcp_errors_show_the_hint_in_the_text(outbox):
    mcp_flat = McpClient(["mcp", "--mcp-flat", *ALLOW], env=outbox)
    mcp_flat.initialize()
    result = mcp_flat.tool("massing_add", {"footprint": [[0, 0], [1000, 0], [1000, 1000]], "bogus": 1})
    assert result["isError"] is True
    mcp_flat.close()
    text = result["content"][0]["text"]
    assert "Draft: {" in text and "feature_request" in text


def test_review_needs_a_terminal():
    out = run_cli("feedback", "review")
    assert out.returncode != 0
    assert "terminal" in out.stderr


def test_a_panic_is_not_reachable_from_outside_but_the_list_counts_people(cad):
    listing = cad.result("feedback.list")
    assert listing["reports_of_people"] == 0 and listing["reports"] == []
