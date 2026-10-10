"""Scenario: the CLI forwards to a running app. A fake app (a TCP server) stands in for the GUI,
so the tests check what the CLI sends first and how it finds the app."""

import json
import os
import socket
import subprocess
import threading

from helpers import BIN, REPO  # noqa: F401


class FakeApp:
    """Accepts one connection. Records the lines it receives and answers each request."""

    def __init__(self, token="s3cret"):
        self.token = token
        self.sock = socket.socket()
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(1)
        self.port = self.sock.getsockname()[1]
        self.lines = []
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()

    def _serve(self):
        conn, _ = self.sock.accept()
        with conn, conn.makefile("rw") as f:
            for line in f:
                req = json.loads(line)
                self.lines.append(req)
                if req["method"] == "auth":
                    ok = req["params"]["token"] == self.token
                    reply = {"jsonrpc": "2.0", "id": req["id"], **({"result": {"ok": True}} if ok else {"error": {"code": -32001, "message": "no"}})}
                    f.write(json.dumps(reply) + "\n")
                    f.flush()
                    if not ok:
                        return
                else:
                    f.write(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": {"echo": req["method"]}}) + "\n")
                    f.flush()


def run_serve(args, requests, env_extra=None):
    env = {**os.environ, **(env_extra or {})}
    p = subprocess.run([str(BIN), "serve", *args], input="".join(json.dumps(r) + "\n" for r in requests),
                       capture_output=True, text=True, timeout=20, env=env)
    return p


def test_connect_with_address_sends_auth_first_with_the_token():
    app = FakeApp()
    p = run_serve(["--connect", f"127.0.0.1:{app.port}", "--token", "s3cret"], [{"jsonrpc": "2.0", "id": 1, "method": "describe"}])
    assert p.returncode == 0, p.stderr
    assert app.lines[0]["method"] == "auth" and app.lines[0]["params"]["token"] == "s3cret"
    assert app.lines[1]["method"] == "describe"
    assert json.loads(p.stdout)["result"] == {"echo": "describe"}


def test_connect_with_a_wrong_token_fails_with_a_message():
    app = FakeApp()
    p = run_serve(["--connect", f"127.0.0.1:{app.port}", "--token", "wrong"], [])
    assert p.returncode == 1
    assert "refused" in p.stderr


def test_connect_with_address_and_no_token_and_no_runtime_file_is_an_error(tmp_path):
    env = {"XDG_RUNTIME_DIR": str(tmp_path), "HOME": str(tmp_path), "LOCALAPPDATA": str(tmp_path)}
    p = run_serve(["--connect", "127.0.0.1:9"], [], env)
    assert p.returncode == 1 and "--token" in p.stderr


def test_connect_without_address_reads_the_runtime_file(tmp_path):
    app = FakeApp(token="from-file")
    folder = tmp_path / "cadektor"
    folder.mkdir()
    (folder / "agent.json").write_text(json.dumps({"port": app.port, "token": "from-file"}))
    env = {"XDG_RUNTIME_DIR": str(tmp_path), "HOME": str(tmp_path), "LOCALAPPDATA": str(tmp_path)}
    p = run_serve(["--connect"], [{"jsonrpc": "2.0", "id": 1, "method": "undo"}], env)
    assert p.returncode == 0, p.stderr
    assert app.lines[0]["params"]["token"] == "from-file"
    assert json.loads(p.stdout)["result"] == {"echo": "undo"}


def test_connect_without_address_and_no_app_running_explains_what_to_do(tmp_path):
    env = {"XDG_RUNTIME_DIR": str(tmp_path), "HOME": str(tmp_path), "LOCALAPPDATA": str(tmp_path)}
    p = run_serve(["--connect"], [], env)
    assert p.returncode == 1 and "no running Cadektor app" in p.stderr


def test_mcp_file_tools_are_forwarded_to_the_app():
    app = FakeApp()
    reqs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "document", "arguments": {"method": "file.save", "params": {"path": "/x"}}}},
    ]
    env = os.environ
    p = subprocess.run([str(BIN), "mcp", "--connect", f"127.0.0.1:{app.port}", "--token", "s3cret"],
                       input="".join(json.dumps(r) + "\n" for r in reqs), capture_output=True, text=True, timeout=20, env=env)
    out = [json.loads(l) for l in p.stdout.splitlines()]
    assert out[0]["result"]["protocolVersion"] == "2025-03-26"
    assert any(l["method"] == "file.save" and l["params"] == {"path": "/x"} for l in app.lines)
