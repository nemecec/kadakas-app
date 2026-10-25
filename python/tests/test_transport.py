"""The TCP transport (with a token) and the address rules.

The test starts a small bridge: it plays the part of the app's agent server. It wants the token
first, then it passes every line to a real ``kadakas serve``."""

from __future__ import annotations

import json
import socket
import subprocess
import threading
import time

import pytest

import kadakas
from kadakas import ConnectionFailed
from kadakas import _rpc

TOKEN = "t" * 64


class Bridge:
    def __init__(self, binary) -> None:
        self.delay = 0.0
        self.server = socket.socket()
        self.server.bind(("127.0.0.1", 0))
        self.server.listen(5)
        self.port = self.server.getsockname()[1]
        self.proc = subprocess.Popen([str(binary), "serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
        self.refused = 0
        self.answers = 0  # answers written to a client, delayed ones too
        threading.Thread(target=self._accept, daemon=True).start()

    def _accept(self) -> None:
        while True:
            try:
                conn, _ = self.server.accept()
            except OSError:
                return
            threading.Thread(target=self._client, args=(conn,), daemon=True).start()

    def _client(self, conn: socket.socket) -> None:
        f = conn.makefile("rw", encoding="utf-8", newline="\n")
        first = json.loads(f.readline())
        if first.get("method") != "auth" or first["params"].get("token") != TOKEN:
            self.refused += 1
            f.write(json.dumps({"jsonrpc": "2.0", "id": first.get("id"), "error": {"code": 1, "message": "refused"}}) + "\n")
            f.flush()
            conn.close()
            return
        f.write(json.dumps({"jsonrpc": "2.0", "id": first["id"], "result": {"ok": True}}) + "\n")
        f.flush()
        for line in f:
            self.proc.stdin.write(line)
            self.proc.stdin.flush()
            answer = self.proc.stdout.readline()
            if self.delay:
                time.sleep(self.delay)
                self.delay = 0.0
            f.write(answer)
            f.flush()
            self.answers += 1

    def close(self) -> None:
        self.server.close()
        self.proc.stdin.close()
        self.proc.wait(timeout=5)


@pytest.fixture
def bridge(kadakas_bin):
    b = Bridge(kadakas_bin)
    yield b
    b.close()


def test_connect_with_token_and_see_live_updates(bridge):
    a = kadakas.connect(f"127.0.0.1:{bridge.port}", TOKEN)
    b = kadakas.connect(f"127.0.0.1:{bridge.port}", TOKEN)
    a.add.circle([0, 0], 5)
    # The second connection sees the change of the first at once: both use one session.
    assert [o.kind for o in b.objects()] == ["circle"]
    b.add.line([0, 0], [1, 1])
    assert a.summary()["object_count"] == 2
    a.close()
    b.close()


def test_wrong_token_is_refused(bridge):
    with pytest.raises(ConnectionFailed, match="refused the token"):
        kadakas.connect(f"127.0.0.1:{bridge.port}", "wrong")
    assert bridge.refused == 1


def test_connect_reads_the_environment_the_app_sets_for_scripts(bridge, monkeypatch):
    monkeypatch.setenv("KADAKAS_ADDR", f"127.0.0.1:{bridge.port}")
    monkeypatch.setenv("KADAKAS_TOKEN", TOKEN)
    with kadakas.connect() as doc:
        assert doc.summary()["object_count"] == 0


def test_nothing_listens(monkeypatch):
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    with pytest.raises(ConnectionFailed, match="cannot connect"):
        kadakas.connect(f"127.0.0.1:{port}", "x")


def test_endpoint_rules(tmp_path, monkeypatch):
    for k in ("KADAKAS_ADDR", "KADAKAS_TOKEN"):
        monkeypatch.delenv(k, raising=False)
    # The runtime folder is named by XDG_RUNTIME_DIR on Unix and by LOCALAPPDATA on Windows.
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path))
    (tmp_path / "kadakas").mkdir()
    # No runtime file: say what to do.
    with pytest.raises(ConnectionFailed, match="--agent-port"):
        _rpc.resolve_endpoint(None, None)
    (tmp_path / "kadakas" / "agent.json").write_text(json.dumps({"port": 7001, "token": "filetoken"}), encoding="utf-8")
    assert _rpc.resolve_endpoint(None, None) == ("127.0.0.1", 7001, "filetoken")
    # The token of the file is only for the app that the file describes.
    assert _rpc.resolve_endpoint("127.0.0.1:7001", None) == ("127.0.0.1", 7001, "filetoken")
    with pytest.raises(ConnectionFailed, match="token"):
        _rpc.resolve_endpoint("10.0.0.5:7001", None)
    assert _rpc.resolve_endpoint("10.0.0.5:7001", "mine") == ("10.0.0.5", 7001, "mine")
    with pytest.raises(ConnectionFailed, match="HOST:PORT"):
        _rpc.resolve_endpoint("nonsense", "t")
    (tmp_path / "kadakas" / "agent.json").write_text("not json", encoding="utf-8")
    with pytest.raises(ConnectionFailed):
        _rpc.resolve_endpoint(None, None)
    assert _rpc.agent_info_path() == tmp_path / "kadakas" / "agent.json"


def test_find_binary_rules(monkeypatch):
    monkeypatch.setenv("KADAKAS_BIN", "/from/env")
    assert _rpc.find_binary() == "/from/env"
    assert _rpc.find_binary("/explicit") == "/explicit"
    monkeypatch.delenv("KADAKAS_BIN")
    monkeypatch.setenv("PATH", "")
    with pytest.raises(ConnectionFailed, match="cannot find"):
        _rpc.find_binary()


def test_a_dead_process_is_reported(kadakas_bin):
    t = _rpc.StdioTransport([], kadakas_bin)
    t._proc.kill()
    t._proc.wait()
    with pytest.raises(ConnectionFailed, match="ended"):
        t.request("describe")
    t.close()


def test_a_timeout_does_not_break_the_connection(bridge):
    # Connect and make the first call with a long timeout (a busy machine is slow), then use a
    # short one for the call that must time out.
    doc = kadakas.connect(f"127.0.0.1:{bridge.port}", TOKEN, timeout=30)
    doc.add.circle([0, 0], 5)
    doc._transport._timeout = 0.4
    doc._transport._sock.settimeout(0.4)
    bridge.delay = 1.0  # the next answer comes late
    sent = bridge.answers
    with pytest.raises(kadakas.RequestTimeout):
        doc.summary()
    # Wait until the late answer is on the wire (not a fixed pause: a busy machine is slower).
    deadline = time.monotonic() + 30
    while bridge.answers == sent:
        assert time.monotonic() < deadline, "the bridge never sent the late answer"
        time.sleep(0.01)
    # The late answer is thrown away by its ID. The next call gets its own answer. The timeout
    # of this call is long, so that a busy machine does not turn it into a second timeout.
    doc._transport._timeout = 30.0
    doc._transport._sock.settimeout(30.0)
    assert doc.summary()["object_count"] == 1
    assert [o.kind for o in doc.objects()] == ["circle"]
    doc.close()


def test_a_late_answer_that_arrives_during_the_next_call_is_skipped(bridge):
    doc = kadakas.connect(f"127.0.0.1:{bridge.port}", TOKEN, timeout=30)
    doc._transport._timeout = 0.4
    doc._transport._sock.settimeout(0.4)
    bridge.delay = 0.7
    with pytest.raises(kadakas.RequestTimeout):
        doc.summary()
    # No pause: the next call reads the old answer first, skips it, and waits for its own.
    doc._transport._timeout = 30.0
    doc._transport._sock.settimeout(30.0)
    assert doc.summary()["object_count"] == 0
    doc.close()
