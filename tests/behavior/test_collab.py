"""Scenario: an agent checks the collaboration methods (PLAT-07, PLAT-08).

The CLI gives the session no collaboration endpoint: a host decides where agents may connect. So an
agent can read the status and the documentation, and gets clear errors when it tries to connect.
The full flow (two sessions, a server, undo, presence) is tested in the Rust tests with the
in-memory reference sequencer."""

COLLAB_METHODS = [
    "collab.status", "collab.connect", "collab.disconnect", "collab.sync",
    "collab.presence", "collab.undo", "collab.redo",
]


def test_describe_lists_the_collaboration_methods(cad):
    desc = cad.result("describe")
    names = [m["name"] for m in desc["methods"]]
    assert set(COLLAB_METHODS) <= set(names)
    assert "last edit in server order wins" in desc["collaboration"]["model"]


def test_status_without_a_connection(cad):
    status = cad.result("collab.status")
    assert status["state"] == "none"
    assert status["endpoint"] is None


def test_connect_is_refused_when_the_host_gave_no_endpoint(cad):
    error = cad.error("collab.connect", {"name": "Ann"})
    assert error["data"]["kind"] == "policy"
    assert "endpoint" in error["message"]
    # The session keeps working, and nothing is connected.
    assert cad.result("collab.status")["state"] == "none"


def test_other_methods_need_a_connection(cad):
    for method in ["collab.sync", "collab.presence", "collab.undo", "collab.redo"]:
        error = cad.error(method, {"now_ms": 1})
        assert error["data"]["kind"] == "conflict", method
    assert cad.result("collab.disconnect")["state"] == "none"
