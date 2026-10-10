"""JSON-RPC transports: a child ``cadektor serve`` process (stdio) or a running app (TCP)."""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any, Protocol

from .errors import CadektorError, ConnectionFailed


class Transport(Protocol):
    """Anything that can send one request and return the result."""

    def request(self, method: str, params: dict[str, Any] | None = None) -> Any: ...

    def close(self) -> None: ...


def agent_info_path() -> Path | None:
    """Where the app writes its address and token (same rule as ``cadektor_core::fsio``)."""
    env = os.environ
    if sys.platform == "win32":
        base = env.get("LOCALAPPDATA")
        return Path(base) / "cadektor" / "agent.json" if base else None
    if env.get("XDG_RUNTIME_DIR"):
        return Path(env["XDG_RUNTIME_DIR"]) / "cadektor" / "agent.json"
    home = env.get("HOME")
    if not home:
        return None
    if sys.platform == "darwin":
        return Path(home) / "Library" / "Application Support" / "cadektor" / "agent.json"
    return Path(home) / ".local" / "state" / "cadektor" / "agent.json"


def find_binary(binary: str | os.PathLike[str] | None = None) -> str:
    """The ``cadektor`` program: the argument, ``$CADEKTOR_BIN``, or ``cadektor`` on PATH."""
    found = binary or os.environ.get("CADEKTOR_BIN") or shutil.which("cadektor")
    if not found:
        raise ConnectionFailed(
            "cannot find the cadektor program. Put it on PATH, or set CADEKTOR_BIN, "
            "or pass binary=... to cadektor.open()"
        )
    return str(found)


class _LineRpc:
    """Shared request logic for transports that read and write one JSON object per line."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._next_id = 1

    def _send_line(self, line: str) -> None:  # pragma: no cover - overridden
        raise NotImplementedError

    def _read_line(self) -> str:  # pragma: no cover - overridden
        raise NotImplementedError

    def request(self, method: str, params: dict[str, Any] | None = None) -> Any:
        with self._lock:
            req_id = self._next_id
            self._next_id += 1
            message: dict[str, Any] = {"jsonrpc": "2.0", "id": req_id, "method": method}
            if params is not None:
                message["params"] = params
            self._send_line(json.dumps(message, allow_nan=False))
            reply = json.loads(self._read_line())
        if reply.get("id") != req_id:
            raise CadektorError(-32603, f"reply {reply.get('id')!r} does not match request {req_id}")
        if "error" in reply:
            err = reply["error"]
            raise CadektorError(err.get("code", 0), err.get("message", "error"), method=method)
        return reply.get("result")


class StdioTransport(_LineRpc):
    """Starts ``cadektor serve`` and talks to it over its standard input and output."""

    def __init__(self, args: list[str], binary: str | os.PathLike[str] | None = None) -> None:
        super().__init__()
        program = find_binary(binary)
        try:
            self._proc = subprocess.Popen(
                [program, "serve", *args],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                text=True,
                encoding="utf-8",
                bufsize=1,
            )
        except OSError as e:
            raise ConnectionFailed(f"cannot start {program}: {e}") from e

    def _send_line(self, line: str) -> None:
        assert self._proc.stdin is not None
        try:
            self._proc.stdin.write(line + "\n")
            self._proc.stdin.flush()
        except (BrokenPipeError, OSError) as e:
            raise ConnectionFailed("the cadektor process has ended") from e

    def _read_line(self) -> str:
        assert self._proc.stdout is not None
        line = self._proc.stdout.readline()
        if not line:
            raise ConnectionFailed("the cadektor process has ended")
        return line

    def close(self) -> None:
        if self._proc.poll() is None:
            if self._proc.stdin:
                self._proc.stdin.close()
            try:
                self._proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._proc.kill()
                self._proc.wait()
        if self._proc.stdout:
            self._proc.stdout.close()


class TcpTransport(_LineRpc):
    """Connects to a running app. The first message is ``auth`` with the token."""

    def __init__(self, host: str, port: int, token: str, timeout: float | None = 120.0) -> None:
        super().__init__()
        try:
            self._sock = socket.create_connection((host, port), timeout=timeout)
        except OSError as e:
            raise ConnectionFailed(f"cannot connect to {host}:{port}: {e}") from e
        self._file = self._sock.makefile("rw", encoding="utf-8", newline="\n")
        try:
            self.request("auth", {"token": token})
        except (CadektorError, ConnectionFailed, json.JSONDecodeError) as e:
            self.close()
            raise ConnectionFailed(f"{host}:{port} refused the token") from e

    def _send_line(self, line: str) -> None:
        try:
            self._file.write(line + "\n")
            self._file.flush()
        except OSError as e:
            raise ConnectionFailed("the connection to the app is closed") from e

    def _read_line(self) -> str:
        try:
            line = self._file.readline()
        except OSError as e:
            raise ConnectionFailed("the connection to the app is closed") from e
        if not line:
            raise ConnectionFailed("the connection to the app is closed")
        return line

    def close(self) -> None:
        for closer in (self._file.close, self._sock.close):
            try:
                closer()
            except OSError:
                pass


def resolve_endpoint(addr: str | None, token: str | None) -> tuple[str, int, str]:
    """Finds ``(host, port, token)``: from the arguments, from ``CADEKTOR_ADDR`` and
    ``CADEKTOR_TOKEN`` (set for scripts that the app starts), or from the app's runtime file."""
    addr = addr or os.environ.get("CADEKTOR_ADDR")
    token = token or os.environ.get("CADEKTOR_TOKEN")
    if addr is None or token is None:
        path = agent_info_path()
        try:
            info = json.loads(path.read_text(encoding="utf-8")) if path else None
        except (OSError, ValueError):
            info = None
        if info is None and addr is None:
            raise ConnectionFailed(
                "no running Cadektor app found. Start the app with --agent-port, "
                "or pass addr=HOST:PORT and token=..."
            )
        file_addr = f"127.0.0.1:{info['port']}" if info else None
        if addr is None:
            addr = file_addr
        # The token in the file is only for the app that the file describes.
        if token is None and info is not None and addr == file_addr:
            token = info.get("token")
    if token is None:
        raise ConnectionFailed("a token is needed with an explicit address")
    host, _, port = addr.rpartition(":")
    if not host or not port.isdigit():
        raise ConnectionFailed(f"address {addr!r} must look like HOST:PORT")
    return host, int(port), token
