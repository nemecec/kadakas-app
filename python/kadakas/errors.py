"""Errors of the ``kadakas`` package."""

from __future__ import annotations


class KadakasError(Exception):
    """The session refused a request.

    ``code`` is the error code and ``kind`` is the stable string of ``data.kind`` (see
    ``describe()["errors"]``). The codes: -32700 parse error, -32600 invalid request, -32601 unknown
    method, -32602 invalid parameters, 1 other failure, 2 not found, 3 validation, 4 policy refused,
    5 conflict, 6 limit exceeded. ``KadakasError(code, message)`` returns the subclass for the code,
    so ``except NotFound`` works. Catch ``KadakasError`` to catch them all."""

    def __new__(cls, code: int = 1, message: str = "", method: str | None = None,
                kind: str | None = None, data: dict | None = None):
        if cls is KadakasError:
            cls = _BY_CODE.get(code, KadakasError)
        return super().__new__(cls)

    def __init__(self, code: int, message: str, method: str | None = None,
                 kind: str | None = None, data: dict | None = None) -> None:
        prefix = f"{method}: " if method else ""
        super().__init__(f"{prefix}{message}")
        self.code = code
        self.message = message
        self.method = method
        self.kind = kind or KINDS.get(code, "error")
        #: More facts of the error (``data`` of the reply without ``kind``), for example the list of
        #: versions when a save is refused because of a limit.
        self.data = {k: v for k, v in (data or {}).items() if k != "kind"}

    @property
    def is_forbidden(self) -> bool:
        """True if the host does not allow this (a policy refusal, for example agent scripts)."""
        return self.code in (POLICY, -32001)


GENERIC, NOT_FOUND, VALIDATION, POLICY, CONFLICT, LIMIT = 1, 2, 3, 4, 5, 6
PARSE_ERROR, INVALID_REQUEST, METHOD_NOT_FOUND, INVALID_PARAMS = -32700, -32600, -32601, -32602

KINDS = {
    PARSE_ERROR: "parse_error", INVALID_REQUEST: "invalid_request",
    METHOD_NOT_FOUND: "method_not_found", INVALID_PARAMS: "invalid_params", GENERIC: "error",
    NOT_FOUND: "not_found", VALIDATION: "validation", POLICY: "policy", CONFLICT: "conflict",
    LIMIT: "limit",
}


class InvalidParams(KadakasError):
    """Code -32602 (also -32600 and -32700): the request is malformed."""


class UnknownMethod(KadakasError):
    """Code -32601: the method does not exist."""


class NotFound(KadakasError):
    """Code 2: an object, layer, class, resource, file or job does not exist."""


class ValidationError(KadakasError):
    """Code 3: the values are well formed, but the document does not allow them."""


class PolicyRefused(KadakasError):
    """Code 4: the host refuses it (a path, a URL, a script or a converter is not allowed)."""


class Conflict(KadakasError):
    """Code 5: the target is in use or has changed."""


class LimitExceeded(KadakasError):
    """Code 6: a size, count or time limit was exceeded."""


_BY_CODE = {
    PARSE_ERROR: InvalidParams, INVALID_REQUEST: InvalidParams, INVALID_PARAMS: InvalidParams,
    METHOD_NOT_FOUND: UnknownMethod, NOT_FOUND: NotFound, VALIDATION: ValidationError,
    POLICY: PolicyRefused, -32001: PolicyRefused, CONFLICT: Conflict, LIMIT: LimitExceeded,
}


class ConnectionFailed(Exception):
    """The program did not start, or the connection broke."""


class RequestTimeout(TimeoutError):
    """The session did not answer in time. The connection is still good: the late answer is
    thrown away when it comes, and the next request works."""
