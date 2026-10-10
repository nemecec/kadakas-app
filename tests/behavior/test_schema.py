"""Scenario: an agent asks `describe` for the JSON Schema of commands and checks its commands with it.

The checker below is a small validator for the parts of JSON Schema that the schema uses
(type, enum, const, required, properties, items, oneOf, anyOf, $ref, minItems, maxItems, minimum)."""

import json
import re

import pytest


ROOT = {}


def check(schema, value, defs, path="$"):
    """Returns a list of problems (empty if the value fits)."""
    if "$ref" in schema:
        if schema["$ref"] == "#":
            return check(ROOT["schema"], value, defs, path)
        name = schema["$ref"].rsplit("/", 1)[-1]
        return check(defs[name], value, defs, path)
    problems = []
    if "oneOf" in schema or "anyOf" in schema:
        options = schema.get("oneOf") or schema["anyOf"]
        results = [check(o, value, defs, path) for o in options]
        ok = sum(1 for r in results if not r)
        if ok == 0:
            # The best explanation is the option with the fewest problems.
            best = min(results, key=len)
            return [f"{path}: no option fits: {best[:2]}"]
        if "oneOf" in schema and ok > 1:
            return [f"{path}: {ok} options fit, one is expected"]
        return []
    t = schema.get("type")
    types = t if isinstance(t, list) else ([t] if t else [])
    if types:
        def is_type(name):
            return {
                "object": isinstance(value, dict),
                "array": isinstance(value, list),
                "string": isinstance(value, str),
                "integer": isinstance(value, int) and not isinstance(value, bool),
                "number": isinstance(value, (int, float)) and not isinstance(value, bool),
                "boolean": isinstance(value, bool),
                "null": value is None,
            }[name]
        if not any(is_type(n) for n in types):
            return [f"{path}: expected {types}, got {type(value).__name__}"]
    if "const" in schema and value != schema["const"]:
        problems.append(f"{path}: expected {schema['const']!r}")
    if "enum" in schema and value not in schema["enum"]:
        problems.append(f"{path}: {value!r} is not one of {schema['enum']}")
    if "minimum" in schema and isinstance(value, (int, float)) and value < schema["minimum"]:
        problems.append(f"{path}: below {schema['minimum']}")
    if isinstance(value, dict):
        for key in schema.get("required", []):
            if key not in value:
                problems.append(f"{path}: missing {key}")
        for key, sub in schema.get("properties", {}).items():
            if key in value:
                problems += check(sub, value[key], defs, f"{path}.{key}")
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0) or len(value) > schema.get("maxItems", 1 << 30):
            problems.append(f"{path}: wrong number of items")
        if "items" in schema:
            for i, item in enumerate(value):
                problems += check(schema["items"], item, defs, f"{path}[{i}]")
    return problems


@pytest.fixture
def schema(cad):
    desc = cad.result("describe", {"schema": True})
    return desc["command_schema"]


def validate(schema, command):
    ROOT["schema"] = schema
    return check(schema, command, schema["$defs"])


def test_describe_only_adds_the_schema_on_request(cad):
    plain = cad.result("describe")
    assert "command_schema" not in plain
    assert "schemas" in plain and "schema" in plain["schemas"]["command"]
    full = cad.result("describe", {"schema": True})
    assert full["command_schema"]["$schema"].endswith("2020-12/schema")


def test_the_schema_is_stable_between_calls(cad):
    a = json.dumps(cad.result("describe", {"schema": True})["command_schema"], sort_keys=False)
    b = json.dumps(cad.result("describe", {"schema": True})["command_schema"], sort_keys=False)
    assert a == b


def test_every_command_example_fits_the_schema(cad, schema):
    examples = cad.result("describe")["command_examples"]
    assert len(examples) >= 10
    for example in examples:
        assert validate(schema, example) == [], example


def test_the_schema_lists_the_ops_and_rejects_bad_commands(schema):
    ops = {o["properties"]["op"]["const"] for o in schema["oneOf"]}
    assert {"add_object", "move_objects", "transform_objects", "array_objects", "set_visibility", "batch"} <= ops
    assert validate(schema, {"op": "move_objects", "ids": [1], "delta": [1, 2]}) == []
    assert validate(schema, {"op": "move_objects", "ids": [1]})  # delta is missing
    assert validate(schema, {"op": "move_objects", "ids": [1], "delta": [1]})  # a point has two numbers
    assert validate(schema, {"op": "move_objects", "ids": ["a"], "delta": [1, 2]})
    assert validate(schema, {"op": "no_such_command"})
    assert validate(schema, {"op": "add_object", "shape": {"kind": "teapot"}})
    assert validate(schema, {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1, 1]}}) == []


def test_new_commands_fit_the_schema(cad, schema):
    cmds = [
        {"op": "transform_objects", "ids": [2], "transform": {"type": "scale", "center": [0, 0], "sx": 2}, "copy": True},
        {"op": "array_objects", "ids": [2], "array": {"shape": "circular", "count": 3, "center": [0, 0], "retain": False}},
        {"op": "set_visibility", "layers": [1], "mode": "grey"},
        {"op": "batch", "commands": [{"op": "delete_objects", "ids": [2]}, {"op": "set_visibility", "classes": [0], "mode": "invisible"}]},
    ]
    for c in cmds:
        assert validate(schema, c) == [], c
    assert validate(schema, {"op": "set_visibility", "mode": "pink"})
    assert validate(schema, {"op": "transform_objects", "ids": [2], "transform": {"type": "tilt"}})
    # What the schema accepts, the server runs.
    cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [10, 0], "radius": 2}})
    for c in cmds[:3]:
        resp = cad.rpc("execute", {"command": c})
        assert "error" not in resp, (c, resp)


def test_schema_ops_match_what_the_server_knows(cad, schema):
    # An op that is not in the schema is refused by the server with the list of known ones.
    resp = cad.rpc("execute", {"command": {"op": "no_such_command"}})
    text = resp["error"]["message"]
    known = set(re.findall(r"`([a-z_0-9]+)`", text.split("expected", 1)[1]))
    assert known == {o["properties"]["op"]["const"] for o in schema["oneOf"]}
