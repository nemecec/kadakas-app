"""JSON-RPC transports: a child ``cadektor serve`` process (stdio) or a running app (TCP)."""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
import threading
import time
from collections import deque
from pathlib import Path
from typing import Any, Callable, Protocol

from .errors import CadektorError, ConnectionFailed, RequestTimeout


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


NOTIFICATION_LIMIT = 10_000
"""How many notifications a transport keeps for a client that does not take them."""


class _LineRpc:
    """Shared request logic for transports that read and write one JSON object per line."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._next_id = 1
        self._abandoned: set[Any] = set()
        # Messages without an id (``events.change``), oldest first. The newest ones stay.
        self._notes: deque[dict[str, Any]] = deque(maxlen=NOTIFICATION_LIMIT)
        self._listeners: list[Callable[[dict[str, Any]], None]] = []

    def add_listener(self, callback: Callable[[dict[str, Any]], None]) -> None:
        """Call ``callback(message)`` for each notification, in the thread that reads it."""
        self._listeners.append(callback)

    def drain_notifications(self) -> list[dict[str, Any]]:
        """The notifications that arrived and were not taken yet, oldest first."""
        with self._lock:
            notes = list(self._notes)
            self._notes.clear()
        return notes

    def wait_notifications(self, timeout: float | None = None) -> list[dict[str, Any]]:
        """Like ``drain_notifications``, but waits up to ``timeout`` seconds for one if there is none.
        Only a transport that can read without a request waits (TCP). Others return at once."""
        with self._lock:
            if not self._notes:
                self._wait_for_note(timeout)
        return self.drain_notifications()

    def _wait_for_note(self, timeout: float | None) -> None:
        """Reads until a notification is queued or the time is over. Called with the lock held."""

    def _note(self, message: dict[str, Any]) -> None:
        self._notes.append(message)
        for callback in list(self._listeners):
            callback(message)

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
            try:
                while True:
                    reply = json.loads(self._read_line())
                    if "method" in reply and "id" not in reply:
                        self._note(reply)
                        continue
                    if reply.get("id") in self._abandoned:
                        # The late answer of a request that timed out. Throw it away.
                        self._abandoned.discard(reply.get("id"))
                        continue
                    break
            except RequestTimeout:
                self._abandoned.add(req_id)
                raise
        if reply.get("id") != req_id:
            raise CadektorError(-32603, f"reply {reply.get('id')!r} does not match request {req_id}")
        if "error" in reply:
            err = reply["error"]
            raise CadektorError(err.get("code", 0), err.get("message", "error"), method=method,
                                kind=(err.get("data") or {}).get("kind"))
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
    """Connects to a running app. The first message is ``auth`` with the token.

    ``timeout`` is the time to wait for one answer, in seconds (``None``: wait for ever). After a
    timeout the request raises ``RequestTimeout``. The connection stays usable: the late answer is
    discarded by its ID."""

    def __init__(self, host: str, port: int, token: str, timeout: float | None = 120.0) -> None:
        super().__init__()
        self._timeout = timeout
        self._buffer = bytearray()
        try:
            self._sock = socket.create_connection((host, port), timeout=timeout)
        except OSError as e:
            raise ConnectionFailed(f"cannot connect to {host}:{port}: {e}") from e
        try:
            self.request("auth", {"token": token})
        except (CadektorError, ConnectionFailed, RequestTimeout, json.JSONDecodeError) as e:
            self.close()
            raise ConnectionFailed(f"{host}:{port} refused the token") from e

    def _send_line(self, line: str) -> None:
        try:
            self._sock.sendall(line.encode("utf-8") + b"\n")
        except OSError as e:
            raise ConnectionFailed("the connection to the app is closed") from e

    def _read_line(self) -> str:
        # Our own buffer: a timeout in the middle of a line must not lose the bytes that came.
        while True:
            end = self._buffer.find(b"\n")
            if end >= 0:
                line = bytes(self._buffer[:end])
                del self._buffer[: end + 1]
                return line.decode("utf-8")
            try:
                chunk = self._sock.recv(65536)
            except (TimeoutError, socket.timeout) as e:
                raise RequestTimeout(f"no answer in {self._timeout} s") from e
            except OSError as e:
                raise ConnectionFailed("the connection to the app is closed") from e
            if not chunk:
                raise ConnectionFailed("the connection to the app is closed")
            self._buffer += chunk

    def _wait_for_note(self, timeout: float | None) -> None:
        old = self._sock.gettimeout()
        deadline = None if timeout is None else time.monotonic() + timeout
        try:
            while not self._notes:
                left = None if deadline is None else max(deadline - time.monotonic(), 0.0)
                if left == 0.0:
                    return
                self._sock.settimeout(left)
                try:
                    message = json.loads(self._read_line())
                except RequestTimeout:
                    return
                if "method" in message and "id" not in message:
                    self._note(message)
                # A reply here is the late answer of a request that timed out: drop it.
        finally:
            self._sock.settimeout(old)

    def close(self) -> None:
        try:
            self._sock.close()
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
    assert addr is not None  # set above: from the argument, the environment or the file
    host, _, port = addr.rpartition(":")
    if not host or not port.isdigit():
        raise ConnectionFailed(f"address {addr!r} must look like HOST:PORT")
    return host, int(port), token
