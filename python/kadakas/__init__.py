"""Kadakas from Python.

::

    import kadakas

    with kadakas.open() as doc:            # starts `kadakas serve`
        doc.add.line([0, 0], [5000, 0])
        with doc.undo_group():              # one undo step
            doc.add.circle([0, 0], 300)
            doc.add.circle([1000, 0], 300)
        print(doc.summary()["objects_by_kind"])

    doc = kadakas.connect()                # a running app (or KADAKAS_ADDR and KADAKAS_TOKEN)

See ``docs/scripting.md``.
"""

from .document import (
    Class,
    Document,
    Id,
    Layer,
    Obj,
    Pending,
    connect,
    is_id,
    open,
)
from .errors import (
    KadakasError,
    Conflict,
    ConnectionFailed,
    InvalidParams,
    LimitExceeded,
    NotFound,
    PolicyRefused,
    RequestTimeout,
    UnknownMethod,
    ValidationError,
)
from .plugins import Param, PluginObject, Run

__all__ = [
    "KadakasError", "Class", "Conflict", "ConnectionFailed", "Document", "Id", "InvalidParams", "Layer",
    "LimitExceeded", "NotFound", "Obj", "Param", "Pending", "PluginObject", "PolicyRefused",
    "RequestTimeout", "Run", "UnknownMethod", "ValidationError", "connect", "is_id", "open",
]
__version__ = "0.1.0"
