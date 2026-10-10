"""The error codes of the API map to exception classes."""

from __future__ import annotations

import pytest

import cadektor
from cadektor import (
    CadektorError,
    Conflict,
    InvalidParams,
    LimitExceeded,
    NotFound,
    PolicyRefused,
    UnknownMethod,
    ValidationError,
)


@pytest.mark.parametrize(("code", "cls", "kind"), [
    (-32700, InvalidParams, "parse_error"),
    (-32600, InvalidParams, "invalid_request"),
    (-32601, UnknownMethod, "method_not_found"),
    (-32602, InvalidParams, "invalid_params"),
    (2, NotFound, "not_found"),
    (3, ValidationError, "validation"),
    (4, PolicyRefused, "policy"),
    (5, Conflict, "conflict"),
    (6, LimitExceeded, "limit"),
])
def test_codes_map_to_classes(code, cls, kind):
    e = CadektorError(code, "m", "some.method")
    assert type(e) is cls and isinstance(e, CadektorError)
    assert e.code == code and e.kind == kind and e.method == "some.method"
    assert str(e) == "some.method: m"


def test_generic_and_unknown_codes_stay_cadektor_error():
    assert type(CadektorError(1, "x")) is CadektorError
    assert CadektorError(1, "x").kind == "error"
    assert type(CadektorError(77, "x")) is CadektorError
    # The server may send its own kind.
    assert CadektorError(1, "x", kind="special").kind == "special"


def test_policy_refusals_are_forbidden():
    assert CadektorError(4, "no").is_forbidden
    assert CadektorError(-32001, "no").is_forbidden
    assert not CadektorError(2, "no").is_forbidden


def test_a_live_session_raises_the_right_class(doc):
    with pytest.raises(NotFound) as e:
        doc.get(999)
    assert e.value.kind == "not_found" and e.value.code == 2
    with pytest.raises(UnknownMethod):
        doc.call("nothing.here")
    with pytest.raises(ValidationError) as e:
        doc.add.circle([0, 0], -1)
    assert e.value.kind == "validation"
    # Catching the base class still works.
    with pytest.raises(CadektorError):
        doc.get(999)


def test_converters_are_off_unless_allowed(tmp_path):
    dwg = tmp_path / "a.dwg"
    dwg.write_bytes(b"x")
    with cadektor.open(allow_dir=[tmp_path]) as d:
        with pytest.raises(PolicyRefused) as e:
            d.call("import.file", path=str(dwg))
        assert "--allow-converters" in e.value.message
    with cadektor.open(allow_dir=[tmp_path], allow_converters=True) as d:
        with pytest.raises(CadektorError) as e:
            d.call("import.file", path=str(dwg))
        assert "--allow-converters" not in e.value.message
