"""Scenario: a UI or an agent follows the document through the change-event stream instead of polling.

Over `serve`, the notifications (messages without an id) come before the reply of the request that
caused them, in the order of the changes. A batch is one notification."""

import json

from helpers import JsonRpc, rect_cmd, uid


def call(cad, method, params=None):
    """Sends one request and returns (notifications before the reply, the reply)."""
    req_id = cad._next_id
    cad._next_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    cad.send(msg)
    notes = []
    while True:
        line = cad.read_line()
        assert line is not None, f"no response to {method}"
        m = json.loads(line)
        if "id" not in m:
            notes.append(m)
            continue
        assert m["id"] == req_id, m
        return notes, m


def test_changes_arrive_in_order_before_each_reply():
    cad = JsonRpc()
    try:
        notes, reply = call(cad, "events.subscribe")
        assert notes == [] and reply["result"]["revision"] > 0
        notes, reply = call(cad, "execute", {"command": rect_cmd(0, 0, 1, 1)})
        assert len(notes) == 1 and notes[0]["method"] == "events.change"
        change = notes[0]["params"]
        assert change["op"] == "add_object" and change["origin"] == "agent"
        assert change["added"]["objects"] == reply["result"]["created"] == [uid(2)]
        notes, _ = call(cad, "execute", {"command": rect_cmd(2, 2, 3, 3)})
        assert notes[0]["params"]["revision"] > change["revision"]
        notes, _ = call(cad, "undo")
        assert notes[0]["params"]["op"] == "undo"
        assert notes[0]["params"]["removed"]["objects"] == [uid(3)]
    finally:
        cad.close()


def test_a_batch_is_one_notification_and_a_failed_batch_is_none():
    cad = JsonRpc()
    try:
        call(cad, "events.subscribe", {"kinds": ["objects"]})
        notes, _ = call(cad, "execute", {"command": {"op": "batch", "commands": [
            rect_cmd(0, 0, 1, 1), {"op": "add_layer", "name": "Site"}, rect_cmd(5, 5, 6, 6),
        ]}})
        assert len(notes) == 1
        assert notes[0]["params"]["added"]["objects"] == [uid(2), uid(4)]
        assert notes[0]["params"]["added"]["layers"] == []  # the filter keeps only objects
        notes, reply = call(cad, "execute", {"command": {"op": "batch", "commands": [
            rect_cmd(7, 7, 8, 8), {"op": "delete_objects", "ids": [uid(999)]},
        ]}})
        assert "error" in reply and notes == []
        notes, reply = call(cad, "events.unsubscribe")
        assert reply["result"] == {"done": True}
        notes, _ = call(cad, "execute", {"command": rect_cmd(9, 9, 10, 10)})
        assert notes == []
    finally:
        cad.close()


def test_poll_gives_the_same_change_sets():
    cad = JsonRpc()
    try:
        _, start = call(cad, "events.poll", {"since_revision": 0})
        rev = start["result"]["revision"]
        call(cad, "execute", {"command": rect_cmd(0, 0, 1, 1)})
        _, polled = call(cad, "events.poll", {"since_revision": rev})
        assert polled["result"]["truncated"] is False
        assert [c["op"] for c in polled["result"]["changes"]] == ["add_object"]
        _, bad = call(cad, "events.subscribe", {"kinds": ["nope"]})
        assert bad["error"]["code"] == -32602
    finally:
        cad.close()
