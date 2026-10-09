"""Shared helpers for the Cadektor behavior tests. Standard library only."""

from __future__ import annotations

import json
import os
import select
import subprocess
import time
import xml.etree.ElementTree as ET
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get("CADEKTOR_BIN", REPO / "target" / "debug" / "cadektor"))


class StdioClient:
    """A child `cadektor` process that reads one JSON object per line on stdin."""

    def __init__(self, args, timeout=10.0):
        self.timeout = timeout
        self.proc = subprocess.Popen(
            [str(BIN), *args],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,  # unbuffered, so select() reflects exactly what the child wrote
        )
        self._out = self.proc.stdout.fileno()
        self._next_id = 1

    def send_raw(self, text: str) -> None:
        data = memoryview((text + "\n").encode())
        fd = self.proc.stdin.fileno()
        while data:
            data = data[os.write(fd, data):]

    def send(self, obj) -> None:
        self.send_raw(json.dumps(obj))

    def read_line(self, timeout=None) -> str | None:
        """Returns the next output line, or None if nothing arrives in time."""
        deadline = time.monotonic() + (self.timeout if timeout is None else timeout)
        buf = bytearray()
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            ready, _, _ = select.select([self._out], [], [], remaining)
            if not ready:
                return None
            chunk = os.read(self._out, 1)
            if not chunk:
                err = self.proc.stderr.read().decode(errors="replace")
                raise AssertionError(f"cadektor exited early. stderr: {err}")
            if chunk == b"\n":
                return buf.decode()
            buf += chunk

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
        for stream in (self.proc.stdout, self.proc.stderr):
            stream.close()


class JsonRpc(StdioClient):
    """`cadektor serve` client. `rpc` checks that the response id matches the request id."""

    def __init__(self, args=("serve",), **kw):
        super().__init__(list(args), **kw)

    def rpc(self, method, params=None) -> dict:
        req_id = self._next_id
        self._next_id += 1
        msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params is not None:
            msg["params"] = params
        self.send(msg)
        line = self.read_line()
        assert line is not None, f"no response to {method}"
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
    """`cadektor mcp` client speaking MCP over stdio."""

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

    def initialize(self) -> dict:
        resp = self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}})
        assert "result" in resp, resp
        self.notify("notifications/initialized")
        return resp["result"]


def run_cli(*args, timeout=60):
    return subprocess.run(
        [str(BIN), *args], stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=timeout
    )


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
