"""The change-event stream from Python: messages come before the reply of the call that caused
them, a batch is one message, kinds filter, origins say who changed, and a document load resets."""

from __future__ import annotations

import cadektor


def test_each_call_gives_its_change_set_in_order(doc):
    doc.events.subscribe()
    a = doc.add.line([0, 0], [10, 0])
    b = doc.add.circle([0, 0], 5)
    notes = doc.events.take()
    assert [n["method"] for n in notes] == ["events.change", "events.change"]
    assert notes[0]["params"]["added"]["objects"] == [a.id]
    assert notes[1]["params"]["added"]["objects"] == [b.id]
    assert notes[0]["params"]["origin"] == "agent"
    assert notes[0]["params"]["revision"] < notes[1]["params"]["revision"]
    assert doc.events.take() == []


def test_an_undo_group_is_one_message_and_a_failed_one_is_none(doc):
    doc.events.subscribe()
    with doc.undo_group():
        doc.add.circle([0, 0], 1)
        doc.add.circle([5, 0], 1)
        doc.add.circle([9, 0], 1)
    notes = doc.events.take()
    assert len(notes) == 1
    assert len(notes[0]["params"]["added"]["objects"]) == 3
    try:
        doc.call("execute", command={"op": "delete_objects", "ids": ["00000000-0000-0000-0000-0000000000ff"]})
    except cadektor.CadektorError:
        pass
    assert doc.events.take() == []


def test_kinds_callback_and_unsubscribe(doc):
    seen = []
    doc.events.subscribe(kinds=["layers"], callback=seen.append)
    doc.add.circle([0, 0], 1)
    doc.call("execute", command={"op": "add_layer", "name": "Planting", "elevation": 0})
    assert len(seen) == 1 and seen[0]["params"]["added"]["layers"]
    assert len(doc.events.take()) == 1
    assert doc.events.unsubscribe() is True
    doc.add.circle([2, 0], 1)
    assert doc.events.take() == []


def test_poll_and_reset(doc):
    start = doc.events.subscribe()
    doc.add.circle([0, 0], 1)
    r = doc.events.poll(start)
    assert r["truncated"] is False and len(r["changes"]) == 1
    doc.events.take()
    doc.call("document.load", document=doc.call("document.get"))
    notes = doc.events.take()
    assert [n["method"] for n in notes] == ["events.reset"]
    doc.add.circle([4, 0], 1)
    assert len(doc.events.take()) == 1


def test_tcp_transport_waits_for_a_pushed_message():
    """The app pushes the changes that the user makes, with no request open."""
    import json
    import socket
    import threading
    import time

    from cadektor import _rpc

    server = socket.socket()
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    note = {"jsonrpc": "2.0", "method": "events.change", "params": {"revision": 7}}

    def app() -> None:
        conn, _ = server.accept()
        f = conn.makefile("rw", encoding="utf-8", newline="\n")
        first = json.loads(f.readline())
        f.write(json.dumps({"jsonrpc": "2.0", "id": first["id"], "result": {"ok": True}}) + "\n")
        f.flush()
        time.sleep(0.2)
        f.write(json.dumps(note) + "\n")
        f.flush()
        time.sleep(1)
        conn.close()

    threading.Thread(target=app, daemon=True).start()
    t = _rpc.TcpTransport("127.0.0.1", server.getsockname()[1], "tok", timeout=5)
    assert t.wait_notifications(0.01) == []
    assert t.wait_notifications(3) == [note]
    t.close()
    server.close()
