"""Shared helpers for the Kadakas behavior tests. Standard library only."""

from __future__ import annotations

import json
import os
import queue
import subprocess
import sys
import threading
import xml.etree.ElementTree as ET
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
_EXE = "kadakas.exe" if sys.platform == "win32" else "kadakas"
BIN = Path(os.environ.get("KADAKAS_BIN", REPO / "target" / "debug" / _EXE))


def _make_home() -> Path:
    """A private home for the child processes. The CLI reads and may write the folders of the
    user (config, data, state: the trust list of plug-ins, for one). A test must never see or
    change the real ones, so every child gets this folder as HOME and for all the variables that
    name a user folder."""
    import tempfile

    home = Path(tempfile.mkdtemp(prefix="kadakas-test-home-"))
    for sub in ("config", "data", "state", "cache", "runtime"):
        (home / sub).mkdir()
    return home


TEST_HOME = _make_home()


_UV_CACHE: list[str] = []


def isolated_env(uv: bool = False) -> dict:
    """The environment for a child process: the real one, with every user folder moved.

    A child that runs `uv` keeps the real cache of uv (`uv=True`): the private home has an empty
    one, and every test would download its packages again."""
    env = dict(os.environ)
    if uv and not _UV_CACHE:
        found = subprocess.run(["uv", "cache", "dir"], capture_output=True, text=True, encoding="utf-8", check=False)
        _UV_CACHE.append(env.get("UV_CACHE_DIR") or found.stdout.strip())
    env.update(
        HOME=str(TEST_HOME),
        USERPROFILE=str(TEST_HOME),
        APPDATA=str(TEST_HOME / "data"),
        LOCALAPPDATA=str(TEST_HOME / "state"),
        XDG_CONFIG_HOME=str(TEST_HOME / "config"),
        XDG_DATA_HOME=str(TEST_HOME / "data"),
        XDG_STATE_HOME=str(TEST_HOME / "state"),
        XDG_CACHE_HOME=str(TEST_HOME / "cache"),
        XDG_RUNTIME_DIR=str(TEST_HOME / "runtime"),
    )
    if uv and _UV_CACHE[0]:
        env["UV_CACHE_DIR"] = _UV_CACHE[0]
    return env


class StdioClient:
    """A child `kadakas` process that reads one JSON object per line on stdin.

    A reader thread moves the output lines into a queue, so a read can time out on every system
    (select() works on pipes only on Unix)."""

    def __init__(self, args, timeout=None, env=None):
        # A busy machine (a parallel build) makes calls slow: the limit is set by the environment.
        self.timeout = float(os.environ.get("KADAKAS_TEST_TIMEOUT", "60")) if timeout is None else timeout
        self.proc = subprocess.Popen(
            [str(BIN), *args],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
            env={**isolated_env(), **(env or {})},
        )
        self._lines: queue.Queue = queue.Queue()
        self._err = bytearray()
        self._threads = [
            threading.Thread(target=self._pump_stdout, daemon=True),
            threading.Thread(target=self._pump_stderr, daemon=True),
        ]
        for t in self._threads:
            t.start()
        self._next_id = 1

    def _pump_stdout(self) -> None:
        # Read byte lines. None marks the end of the stream.
        buf = bytearray()
        try:
            while True:
                chunk = self.proc.stdout.read(1)
                if not chunk:
                    break
                if chunk == b"\n":
                    self._lines.put(bytes(buf).decode())
                    buf.clear()
                else:
                    buf += chunk
        except (OSError, ValueError):
            pass
        self._lines.put(None)

    def _pump_stderr(self) -> None:
        try:
            while True:
                chunk = self.proc.stderr.read(4096)
                if not chunk:
                    break
                self._err += chunk
        except (OSError, ValueError):
            pass

    def send_raw(self, text: str) -> None:
        data = memoryview((text + "\n").encode())
        fd = self.proc.stdin.fileno()
        while data:
            data = data[os.write(fd, data):]

    def send(self, obj) -> None:
        self.send_raw(json.dumps(obj))

    def read_line(self, timeout=None) -> str | None:
        """Returns the next output line, or None if nothing arrives in time."""
        try:
            line = self._lines.get(timeout=self.timeout if timeout is None else timeout)
        except queue.Empty:
            return None
        if line is None:
            self._lines.put(None)  # keep the end mark for later reads
            self._threads[1].join(timeout=2)
            raise AssertionError(f"kadakas exited early. stderr: {self._err.decode(errors='replace')}")
        return line

    def expect_silence(self, wait=0.3) -> None:
        line = self.read_line(timeout=wait)
        assert line is None, f"expected no output, got {line!r}"

    def close(self) -> None:
        if self.proc.poll() is None:
            self.proc.stdin.close()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        for t in self._threads:
            t.join(timeout=2)
        for stream in (self.proc.stdout, self.proc.stderr, self.proc.stdin):
            try:
                stream.close()
            except OSError:
                pass


def uid(n: int) -> str:
    """The ID that has the small number `n`. With `--sequential-ids` the server gives out these IDs
    in order (2, 3, ...), and the layer 1 and the class 0 of a new document have them always."""
    return "00000000-0000-0000-0000-%012x" % n


class JsonRpc(StdioClient):
    """`kadakas serve` client. `rpc` checks that the response id matches the request id.

    A `serve` process runs with `--sequential-ids`: new IDs count 2, 3, 4, so a test can name them."""

    def __init__(self, args=("serve",), **kw):
        args = list(args)
        if args and args[0] == "serve" and "--connect" not in args:
            args.insert(1, "--sequential-ids")
        super().__init__(args, **kw)

    def rpc(self, method, params=None) -> dict:
        req_id = self._next_id
        self._next_id += 1
        msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params is not None:
            msg["params"] = params
        self.send(msg)
        line = self.read_line()
        if line is None:
            # No answer in time: do not leave a stuck program behind.
            self.proc.kill()
        assert line is not None, f"no response to {method} in {self.timeout} s"
        resp = json.loads(line)
        assert resp.get("jsonrpc") == "2.0", resp
        assert resp.get("id") == req_id, f"id mismatch for {method}: sent {req_id}, got {resp}"
        assert ("result" in resp) != ("error" in resp), f"need exactly one of result/error: {resp}"
        return resp

    def result(self, method, params=None):
        resp = self.rpc(method, params)
        assert "error" not in resp, f"{method} failed: {resp}"
        return resp["result"]

    def error(self, method, params=None) -> dict:
        resp = self.rpc(method, params)
        assert "error" in resp, f"{method} should have failed: {resp}"
        return resp["error"]

    def notify(self, method, params=None) -> None:
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        self.send(msg)

    def execute(self, command) -> list:
        """Runs one command and returns the created ids."""
        return self.result("execute", {"command": command})["created"]

    def doc(self) -> dict:
        return self.result("document.get")


class McpClient(StdioClient):
    """`kadakas mcp` client speaking MCP over stdio."""

    def __init__(self, args=("mcp",), **kw):
        super().__init__(list(args), **kw)
        self._mcp_id = 1

    def request(self, method, params=None) -> dict:
        req_id = self._mcp_id
        self._mcp_id += 1
        msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params is not None:
            msg["params"] = params
        self.send(msg)
        line = self.read_line()
        assert line is not None, f"no response to {method}"
        resp = json.loads(line)
        assert resp.get("id") == req_id, f"id mismatch for {method}: {resp}"
        return resp

    def notify(self, method, params=None) -> None:
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        self.send(msg)

    def tool(self, name, arguments=None) -> dict:
        """Calls a tool and returns its MCP result object (content + isError)."""
        resp = self.request("tools/call", {"name": name, "arguments": arguments or {}})
        assert "error" not in resp, f"protocol error for tool {name}: {resp}"
        return resp["result"]

    def tool_json(self, name, arguments=None):
        result = self.tool(name, arguments)
        assert result["isError"] is False, result
        return json.loads(result["content"][0]["text"])

    def method(self, domain, method, params=None) -> dict:
        """Calls an API method through a domain tool and returns its MCP result object."""
        return self.tool(domain, {"method": method, **({"params": params} if params is not None else {})})

    def method_json(self, domain, method, params=None):
        result = self.method(domain, method, params)
        assert result["isError"] is False, result
        return json.loads(result["content"][0]["text"])

    def initialize(self) -> dict:
        resp = self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}})
        assert "result" in resp, resp
        self.notify("notifications/initialized")
        return resp["result"]


def run_cli(*args, timeout=60):
    return subprocess.run(
        [str(BIN), *args],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True, encoding="utf-8",
        timeout=timeout,
        env=isolated_env(),
    )


def pdftotext(path, *options) -> str:
    """The text of a PDF from poppler's ``pdftotext``, as UTF-8 on every system. The output goes to a
    file, not to the pipe (the Windows build does not write ``-bbox`` to standard output).
    Skips the test if the tool is not installed, and says so."""
    import shutil
    import tempfile

    import pytest

    tool = shutil.which("pdftotext")
    if tool is None:
        pytest.skip("pdftotext (poppler) is not installed")
    if "-bbox" in options:
        # Xpdf's pdftotext (the one on the Windows runner) has no -bbox; poppler's has.
        help_text = subprocess.run(
            [tool, "-h"], capture_output=True, text=True, encoding="utf-8", errors="replace"
        )
        if "-bbox" not in help_text.stdout + help_text.stderr:
            pytest.skip("this pdftotext (Xpdf) has no -bbox; poppler's pdftotext is needed")
    out = Path(tempfile.mkdtemp(prefix="kadakas-pdftotext-")) / "out.txt"
    run = subprocess.run(
        [tool, "-enc", "UTF-8", *options, str(path), str(out)],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    assert run.returncode == 0, f"pdftotext exit {run.returncode}: {run.stdout}{run.stderr}"
    return out.read_text(encoding="utf-8")


def read_doc(path) -> dict:
    """The document JSON of a .kad file, whatever its form: the plain JSON form is read as it is,
    a container is turned into JSON by ``kadakas convert`` (the pictures inline, the versions in
    the field ``history``)."""
    import tempfile

    data = Path(path).read_bytes()
    if data.lstrip()[:1] == b"{":
        return json.loads(data)
    out = Path(tempfile.mkdtemp(prefix="kadakas-read-doc-")) / "doc.json"
    p = run_cli("convert", str(path), str(out), "--format", "json")
    assert p.returncode == 0, p.stderr
    return json.loads(out.read_text(encoding="utf-8"))


def svg_object_ids(svg_text: str) -> list[str]:
    """Each drawn object carries data-id on exactly one element."""
    root = ET.fromstring(svg_text)
    return [el.get("data-id") for el in root.iter() if el.get("data-id") is not None]


def parse_svg(svg_text: str) -> ET.Element:
    root = ET.fromstring(svg_text)
    assert root.tag.endswith("svg"), root.tag
    return root


def rect_cmd(x0, y0, x1, y1, **extra) -> dict:
    cmd = {"op": "add_object", "shape": {"kind": "rect", "min": [x0, y0], "max": [x1, y1]}}
    cmd.update(extra)
    return cmd


def line_cmd(ax, ay, bx, by, **extra) -> dict:
    cmd = {"op": "add_object", "shape": {"kind": "line", "a": [ax, ay], "b": [bx, by]}}
    cmd.update(extra)
    return cmd


def plant_cmd(x, y, species="Oak", spread=3000, height=8000, **extra) -> dict:
    cmd = {
        "op": "add_object",
        "shape": {"kind": "plant", "at": [x, y], "species": species, "spread": spread, "height": height},
    }
    cmd.update(extra)
    return cmd


def strip_next_id(doc: dict) -> dict:
    """next_id is never rolled back by undo (ids are not reused), so it is ignored in equality checks."""
    return {k: v for k, v in doc.items() if k != "next_id"}
