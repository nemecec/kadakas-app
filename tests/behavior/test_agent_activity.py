"""Scenario: an agent edits, a person reviews (AGR-05, AGR-06). What a client sees of the activity
log of a `kadakas serve` session: the steps, the summary, the version before the session, and the
description. The window (highlight, panel, revert) is tested in the app crate."""


def rect(x):
    return {"command": {"op": "add_object", "shape": {"kind": "rect", "min": [x, 0], "max": [x + 10, 10]}}}


def test_the_method_and_the_concept_are_described(cad):
    desc = cad.result("describe")
    docs = {m["name"]: m["doc"] for m in desc["methods"]}
    assert "agent.activity" in docs and "script run" in docs["agent.activity"]
    review = desc["review"]["agent_activity"]
    assert "Before agent session" in review["concept"]
    assert "never overwritten" in review["revert"]
    schema = cad.result("describe", {"method": "agent.activity"})
    assert {"step", "since", "limit"} <= set(schema["params"]["properties"])


def test_the_steps_of_a_session(cad):
    assert cad.result("agent.activity")["steps"] == []
    a = cad.result("execute", rect(0))["created"][0]
    cad.result("execute", {"command": {"op": "move_objects", "ids": [a], "delta": [5, 0]}})
    cad.result("document.summary")
    log = cad.result("agent.activity")
    assert [s["summary"] for s in log["steps"]] == ["Added 1 object (1 rect)", "Changed 1 object (1 rect)"]
    assert all(s["state"] == "active" for s in log["steps"])
    # The move touched the object that the first step made: the first step cannot be reverted alone.
    assert [s["revertible"] for s in log["steps"]] == [False, True]
    first = cad.result("agent.activity", {"step": log["steps"][0]["id"]})
    assert "changed later" in first["blocked"]
    detail = cad.result("agent.activity", {"step": log["steps"][1]["id"]})
    assert detail["objects"]["changed"] == [a]


def test_the_changes_of_a_session_show_against_a_version(cad):
    # The app saves "Before agent session" by itself; here the version is saved by hand.
    version = cad.result("version.create", {"name": "Before"})["version"]["id"]
    cad.result("execute", rect(0))
    cad.result("execute", rect(50))
    steps = cad.result("agent.activity")["steps"]
    assert [s["summary"] for s in steps][0] == "Changed the versions"
    assert len(steps) == 3
    diff = cad.result("version.diff", {"from": version, "to": "current", "kinds": ["objects"]})
    assert diff["counts"]["objects"]["added"] == 2
    # The picture of the changes: the same compare overlay that the app draws on the plan.
    svg = cad.result("render.svg", {"compare": {"from": version, "to": "current"}})
    assert svg["compare"]["counts"]["added"] == 2


def test_a_new_document_empties_the_log(cad):
    cad.result("execute", rect(0))
    assert len(cad.result("agent.activity")["steps"]) == 1
    doc = cad.result("document.get")
    cad.result("document.load", {"document": doc})
    assert cad.result("agent.activity")["steps"] == []
