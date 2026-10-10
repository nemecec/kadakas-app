"""Sandboxed WebAssembly plug-ins (PLAT-10) through raw JSON-RPC.

The module is the committed example "Raised planter". Its source is in plugins/raised-planter, and
scripts/build-plugin-fixtures.sh rebuilds it. Running a module needs --allow-agent-scripts, or a
user who trusted it in the app."""

import hashlib
import json

import pytest

from helpers import REPO


def call(cad, method, params=None):
    """Sends one request and returns (notifications before the reply, the reply)."""
    req_id = cad._next_id
    cad._next_id += 1
    cad.send({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}})
    notes = []
    while True:
        m = json.loads(cad.read_line())
        if "id" not in m:
            notes.append(m)
            continue
        assert m["id"] == req_id, m
        return notes, m

MODULE = REPO / "crates" / "kadakas-plugin" / "tests" / "fixtures" / "raised_planter.wasm"
PLUGIN = "org.kadakas.raised-planter"
AREA = {"min": [0, 0], "max": [2400, 900]}


def children(rpc, group):
    return [o for o in rpc("objects.list") if o.get("parent") == group]


def test_describe_lists_the_plugin_methods_with_examples(rpc):
    d = rpc("describe")["result"]
    names = {m["name"] for m in d["methods"]}
    assert {"plugins.info", "plugins.list", "plugins.install", "plugins.remove",
            "plugins.place", "plugins.set_params", "plugins.reset", "plugins.object",
            "plugins.preview"} <= names
    assert "sandboxed" in d["plugins"]["summary"]
    assert {e["method"] for e in d["plugins"]["examples"]} <= names


def test_info_names_the_engine_and_the_limits(cad):
    info = cad.result("plugins.info")
    assert info["engine"] == "wasmtime" and info["installed"] == 0
    assert info["limits"]["fuel"] > 0 and info["limits"]["memory_bytes"] > 0


def test_install_lists_the_manifest_and_the_hash_of_the_module(cad):
    got = cad.result("plugins.install", {"path": str(MODULE)})
    assert got["id"] == PLUGIN and got["kinds"] == ["object"] and got["shape_type"] == "rectangle"
    assert got["hash"] == hashlib.sha256(MODULE.read_bytes()).hexdigest()
    assert got["trusted"] is False and got["existing"] is False
    assert cad.result("plugins.install", {"path": str(MODULE)})["existing"] is True
    assert [p["id"] for p in cad.result("plugins.list")] == [PLUGIN]


def test_an_untrusted_module_does_not_run_and_an_agent_cannot_trust_it(cad):
    got = cad.result("plugins.install", {"path": str(MODULE)})
    err = cad.error("plugins.place", {"plugin": PLUGIN, **AREA})
    assert err["code"] == 4 and got["hash"] in err["message"]
    # No API method adds trust: only a user in the app, or the flag of the host.
    assert cad.error("plugins.trust", {"plugin": PLUGIN, "hash": got["hash"]})["code"] == -32601
    assert not any("trust" in m["name"] for m in cad.result("describe")["methods"])
    assert cad.result("document.summary")["object_count"] == 0


def test_place_set_params_and_undo(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    placed = cad.result("plugins.place", {"plugin": PLUGIN, **AREA, "params": {"wall": 120}})
    group = placed["id"]
    assert placed["shapes"] == 3
    kids = children(cad.result, group)
    assert len(kids) == 3 and {o["shape"]["kind"] for o in kids} == {"polyline", "text"}
    info = cad.result("plugins.object", {"id": group})
    assert info["params"]["wall"] == 120 and info["error"] is None and info["installed"] is True
    assert info["stale"] is False

    changed = cad.result("plugins.set_params", {"id": group, "params": {"label": False}})
    assert changed["ok"] and len(changed["children"]) == 2
    assert cad.result("undo")["done"]
    assert len(children(cad.result, group)) == 3
    assert cad.result("undo")["done"]
    assert cad.result("document.summary")["object_count"] == 0


def test_the_change_origin_is_script(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    cad.result("events.subscribe")
    notes, reply = call(cad, "plugins.place", {"plugin": PLUGIN, **AREA})
    assert len(notes) == 1 and notes[0]["params"]["origin"] == "script"
    assert reply["result"]["id"] in notes[0]["params"]["added"]["objects"]


def test_list_tells_trusted_from_runnable(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    entry = cad.result("plugins.list")[0]
    assert entry["trusted"] is False and entry["runnable"] is True


def test_the_module_can_refuse_and_the_old_shapes_stay(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    group = cad.result("plugins.place", {"plugin": PLUGIN, **AREA})["id"]
    res = cad.result("plugins.set_params", {"id": group, "params": {"wall": 500}})
    assert res["ok"] is False and "too small" in res["error"]
    assert len(children(cad.result, group)) == 3
    assert "too small" in cad.result("plugins.object", {"id": group})["error"]
    err = cad.error("plugins.place", {"plugin": PLUGIN, "min": [0, 0], "max": [900, 300]})
    assert err["code"] == 1
    assert cad.result("document.summary")["object_count"] == 4


def test_bad_input_is_refused(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    assert cad.error("plugins.place", {"plugin": PLUGIN, **AREA, "params": {"wall": 1}})["code"] == -32602
    assert cad.error("plugins.place", {"plugin": PLUGIN, "at": [0, 0]})["code"] == -32602
    assert cad.error("plugins.place", {"plugin": "nope", **AREA})["code"] == 2
    assert cad.error("plugins.install", {"path": str(MODULE.with_name("missing.wasm"))})["code"] == 4


def test_the_path_policy_covers_install(cad, tmp_path):
    # The tests allow the temp folder and the repository. A file outside both is refused.
    outside = "/etc/hosts.wasm" if not str(tmp_path).startswith("C:") else "C:/Windows/x.wasm"
    assert cad.error("plugins.install", {"path": outside})["code"] == 4
    wrong = tmp_path / "module.bin"
    wrong.write_bytes(MODULE.read_bytes())
    err = cad.error("plugins.install", {"path": str(wrong)})
    assert err["code"] == 4 and ".wasm" in err["message"]
    junk = tmp_path / "junk.wasm"
    junk.write_bytes(b"not a module")
    assert cad.error("plugins.install", {"path": str(junk)})["code"] == -32602


def test_remove_needs_that_no_object_uses_the_plugin(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    group = cad.result("plugins.place", {"plugin": PLUGIN, **AREA})["id"]
    assert "use the plug-in" in cad.error("plugins.remove", {"plugin": PLUGIN})["message"]
    cad.result("execute", {"command": {"op": "delete_objects", "ids": [group]}})
    assert cad.result("plugins.remove", {"plugin": PLUGIN}) == {"ok": True}
    assert cad.result("plugins.list") == []


@pytest.mark.parametrize("method", ["plugins.list", "plugins.info"])
def test_the_methods_are_in_the_scripting_tool_of_mcp(mcp, method):
    assert mcp.method_json("scripting", method) is not None


def test_user_shapes_stay_and_null_puts_the_default_back(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    group = cad.result("plugins.place", {"plugin": PLUGIN, **AREA, "params": {"wall": 100}})["id"]
    cad.result("execute", {"command": {"op": "add_object_in", "parent": group,
                                       "shape": {"kind": "circle", "center": [5, 5], "radius": 5}}})
    cad.result("plugins.set_params", {"id": group, "params": {"wall": None, "label": False}})
    assert len(children(cad.result, group)) == 3  # two shapes of the plug-in and the circle of the user
    assert cad.result("plugins.object", {"id": group})["params"]["wall"] == 150


def test_reset_reports_each_object_and_skips_duplicates(make_serve):
    cad = make_serve("--allow-agent-scripts")
    cad.result("plugins.install", {"path": str(MODULE)})
    group = cad.result("plugins.place", {"plugin": PLUGIN, **AREA})["id"]
    missing = "00000000-0000-0000-0000-0000000000ff"
    res = cad.result("plugins.reset", {"ids": [missing, group, group]})["results"]
    assert [r["ok"] for r in res] == [False, True]
