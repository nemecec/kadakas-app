"""Scenario: an agent sends bad input. The server must answer with the right JSON-RPC error and keep working."""

import json

import pytest

from helpers import rect_cmd

# JSON-RPC protocol errors: the request is malformed.
PROTOCOL_ERRORS = {"parse": -32700, "invalid_request": -32600, "method_not_found": -32601, "invalid_params": -32602}
# Application errors (a well-formed request the document cannot accept) use code 1.
APP_ERROR = 1


def raw_response(cad, line: str) -> dict:
    cad.send_raw(line)
    out = cad.read_line()
    assert out is not None, f"no response to {line!r}"
    return json.loads(out)


def test_unparseable_line_gets_parse_error_with_null_id(cad):
    resp = raw_response(cad, "{nope")
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] is None
    assert resp["error"]["code"] == PROTOCOL_ERRORS["parse"]


def test_session_keeps_working_after_a_parse_error(cad):
    raw_response(cad, "{nope")
    assert cad.result("document.summary")["object_count"] == 0


def test_missing_method_is_invalid_request_and_keeps_the_id(cad):
    resp = raw_response(cad, json.dumps({"jsonrpc": "2.0", "id": 7, "params": {}}))
    assert resp["id"] == 7
    assert resp["error"]["code"] == PROTOCOL_ERRORS["invalid_request"]


@pytest.mark.parametrize("method", ["no.such.method", "objects.lists", "EXECUTE"])
def test_unknown_method_is_method_not_found(cad, method):
    error = cad.error(method)
    assert error["code"] == PROTOCOL_ERRORS["method_not_found"]
    assert method in error["message"]


@pytest.mark.parametrize(("method", "params"), [
    ("objects.get", None),
    ("objects.get", {}),
    ("objects.get", {"id": "seven"}),
    ("selection.set", {"ids": "1"}),
    ("execute", {}),
    ("execute", {"command": {"op": "explode_everything"}}),
    ("execute", {"command": {"op": "add_object", "shape": {"kind": "square"}}}),
    ("document.load", {}),
])
def test_bad_params_are_invalid_params(cad, method, params):
    assert cad.error(method, params)["code"] == PROTOCOL_ERRORS["invalid_params"]


@pytest.mark.parametrize(("label", "command"), [
    ("negative circle radius", {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": -1}}),
    ("zero arc radius", {"op": "add_object", "shape": {"kind": "arc", "center": [0, 0], "radius": 0, "start": 0, "end": 1}}),
    ("polyline with one point", {"op": "add_object", "shape": {"kind": "polyline", "points": [[0, 0]], "closed": False}}),
    ("hardscape with two points", {"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [1, 1]], "material": "x"}}),
    ("landscape area with two points", {"op": "add_object", "shape": {"kind": "landscape_area", "boundary": [[0, 0], [1, 1]], "name": "x"}}),
    ("plant with zero spread", {"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 0, "height": 1}}),
    ("text with zero height", {"op": "add_object", "shape": {"kind": "text", "at": [0, 0], "text": "a", "height": 0}}),
    ("unknown layer", rect_cmd(0, 0, 1, 1, layer=999)),
    ("unknown class", rect_cmd(0, 0, 1, 1, **{"class": 999})),
    ("move unknown object", {"op": "move_objects", "ids": [999], "delta": [1, 0]}),
    ("delete unknown object", {"op": "delete_objects", "ids": [999]}),
    ("modify unknown object", {"op": "modify_object", "id": 999, "attrs": {}}),
    ("modify layer unknown id", {"op": "modify_layer", "id": 999, "visible": False}),
    ("modify class unknown id", {"op": "modify_class", "id": 999, "visible": False}),
    ("delete unknown layer", {"op": "delete_layer", "id": 999}),
    ("set active unknown layer", {"op": "set_active", "layer": 999}),
    ("set active unknown class", {"op": "set_active", "class": 999}),
    ("modify layer with zero scale", {"op": "modify_layer", "id": 1, "scale": 0}),
    ("modify layer with negative scale", {"op": "modify_layer", "id": 1, "scale": -2}),
])
def test_invalid_commands_are_application_errors_and_change_nothing(cad, label, command):
    before = cad.doc()
    error = cad.error("execute", {"command": command})
    assert error["code"] == APP_ERROR, label
    assert isinstance(error["message"], str) and error["message"], label
    assert cad.doc() == before, label


@pytest.mark.parametrize(("method", "params", "expected_text"), [
    ("objects.get", {"id": 999}, "999"),
    ("selection.set", {"ids": [999]}, "999"),
])
def test_references_to_unknown_objects_are_application_errors(cad, method, params, expected_text):
    error = cad.error(method, params)
    assert error["code"] == APP_ERROR
    assert expected_text in error["message"]


@pytest.mark.parametrize("line", [
    json.dumps({"jsonrpc": "2.0", "method": "undo"}),
    json.dumps({"jsonrpc": "2.0", "method": "no.such.method"}),
    json.dumps({"jsonrpc": "2.0", "method": "execute", "params": {"command": {"op": "delete_objects", "ids": [999]}}}),
    json.dumps({"jsonrpc": "2.0", "method": "objects.get", "params": {}}),
])
def test_notifications_get_no_response(cad, line):
    cad.send_raw(line)
    cad.expect_silence()
    # The next real request still gets its own, correctly matched, response.
    assert cad.result("document.summary")["object_count"] == 0


def test_notification_that_changes_the_document_still_takes_effect(cad):
    cad.send_raw(json.dumps({"jsonrpc": "2.0", "method": "execute", "params": {"command": rect_cmd(0, 0, 1, 1)}}))
    cad.expect_silence()
    assert cad.result("document.summary")["object_count"] == 1


@pytest.mark.parametrize("scale", [0, -5])
def test_add_layer_rejects_non_positive_scale(cad, scale):
    error = cad.error("execute", {"command": {"op": "add_layer", "name": "Bad", "scale": scale}})
    assert error["code"] == APP_ERROR


def test_new_objects_never_reuse_an_id_after_document_load(cad):
    doc = cad.doc()
    doc["objects"] = [{"id": 5, "layer": 1, "class": 0, "shape": {"kind": "rect", "min": [0, 0], "max": [1, 1]}, "attrs": {}}]
    doc["next_id"] = 2
    cad.result("document.load", {"document": doc})
    new_ids = [cad.execute(rect_cmd(i, i, i + 1, i + 1))[0] for i in range(4)]  # ids 2, 3, 4, 5 would collide
    all_ids = [o["id"] for o in cad.result("objects.list")]
    assert len(set(all_ids)) == len(all_ids), f"duplicate object ids: {all_ids}"
    assert 5 not in new_ids
