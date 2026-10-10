"""The Python package must match the API that `describe` reports.

If this test fails, the API changed. Run ``python python/tools/codegen.py`` to renew
``cadektor/_api.py``, and then check that the wrappers in ``cadektor/document.py`` and
``cadektor/plugins.py`` still fit."""

from __future__ import annotations

import ast
import json
import re
import subprocess
import sys
from pathlib import Path

import pytest

from cadektor import _api
from conftest import BIN, PYTHON_DIR

sys.path.insert(0, str(PYTHON_DIR / "tools"))
import codegen  # noqa: E402


@pytest.fixture(scope="module")
def describe() -> dict:
    return codegen.describe_from_binary(str(BIN))


def test_generated_api_matches_describe(describe):
    expected = codegen.render(describe)
    actual = (PYTHON_DIR / "cadektor" / "_api.py").read_text(encoding="utf-8")
    assert actual == expected, "cadektor/_api.py is out of date. Run python/tools/codegen.py"


def test_every_method_has_a_python_method_with_its_text(describe):
    names = [m["name"] for m in describe["methods"]]
    assert list(_api.METHOD_NAMES) == names
    for m in describe["methods"]:
        fn = getattr(_api.Api, codegen.method_name(m["name"]))
        assert fn.__doc__ is not None and m["doc"].replace("\\", "\\\\").replace('"""', '\\"\\"\\"') in fn.__doc__
    assert _api.UNITS == describe["units"]


def test_the_codegen_check_mode_agrees(describe):
    out = subprocess.run([sys.executable, str(PYTHON_DIR / "tools" / "codegen.py"), "--check", "--binary", str(BIN)],
                         capture_output=True, text=True)
    assert out.returncode == 0, out.stdout + out.stderr


def used_methods() -> tuple[set[str], set[str]]:
    """Method names that the hand-written code uses: `api.x_y(...)` and `call("x.y")`."""
    api_calls: set[str] = set()
    raw_calls: set[str] = set()
    for file in ("document.py", "plugins.py"):
        tree = ast.parse((PYTHON_DIR / "cadektor" / file).read_text(encoding="utf-8"))
        for node in ast.walk(tree):
            if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Attribute) and node.value.attr == "api":
                api_calls.add(node.attr)
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "call":
                if node.args and isinstance(node.args[0], ast.Constant):
                    raw_calls.add(node.args[0].value)
    return api_calls, raw_calls


def test_the_wrappers_only_use_methods_that_exist(describe):
    names = {m["name"] for m in describe["methods"]}
    by_python = {codegen.method_name(n): n for n in names}
    api_calls, raw_calls = used_methods()
    assert len(api_calls) > 40, "the scan found too little: has the code style changed?"
    missing = sorted(a for a in api_calls if a not in by_python)
    assert not missing, f"the wrappers call API methods that describe does not list: {missing}"
    # file.open and file.save are answered by the program (CLI or app) that holds the file paths, so
    # describe does not list them.
    assert sorted(r for r in raw_calls if r not in names and r not in {"file.open", "file.save"}) == []


def test_command_ops_used_by_the_wrappers_are_known(describe):
    """The ops in `{"op": "..."}` of the wrappers must be in the Rust command list (checked through a probe)."""
    ops = set()
    for file in ("document.py", "plugins.py"):
        ops |= set(re.findall(r'"op":\s*"([a-z_]+)"', (PYTHON_DIR / "cadektor" / file).read_text(encoding="utf-8")))
    assert {"add_object", "move_objects", "delete_objects", "batch", "add_layer", "add_class", "modify_layer",
            "modify_class", "modify_object", "set_data", "set_active"} <= ops
    proc = subprocess.Popen([str(BIN), "serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    try:
        for i, op in enumerate(sorted(ops), start=1):
            proc.stdin.write(json.dumps({"id": i, "method": "execute", "params": {"command": {"op": op}}}) + "\n")
            proc.stdin.flush()
            reply = json.loads(proc.stdout.readline())
            # A known op with missing fields is an invalid-params error that names the field, not the op.
            message = reply.get("error", {}).get("message", "")
            assert "unknown variant" not in message, f"{op}: {message}"
    finally:
        proc.stdin.close()
        proc.wait(timeout=5)


def test_the_embedded_runner_is_the_package_file():
    """The Rust crate embeds plugin_runner.py. It must run as a stand-alone program."""
    runner = PYTHON_DIR / "cadektor" / "plugin_runner.py"
    request = {"script": "def reset(p, g):\n    return [line([0, 0], [1, 1])]\n", "params": {}, "geometry": {}}
    out = subprocess.run([sys.executable, "-I", "-c", runner.read_text(encoding="utf-8")],
                         input=json.dumps(request), capture_output=True, text=True, check=True)
    assert json.loads(out.stdout)["shapes"][0]["kind"] == "line"
