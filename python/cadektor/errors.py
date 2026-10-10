"""Errors of the ``cadektor`` package."""

from __future__ import annotations


class CadektorError(Exception):
    """The session refused a request. ``code`` is the JSON-RPC error code."""

    def __init__(self, code: int, message: str, method: str | None = None) -> None:
        prefix = f"{method}: " if method else ""
        super().__init__(f"{prefix}{message}")
        self.code = code
        self.message = message
        self.method = method

    @property
    def is_forbidden(self) -> bool:
        """True if the host does not allow this (for example agent scripts)."""
        return self.code == -32001


class ConnectionFailed(Exception):
    """The program did not start, or the connection broke."""
