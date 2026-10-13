"""Scenario: a designer (or an agent) saves named versions of a garden plan, compares them, goes back
to an earlier one, and finds the versions again after the file is saved and opened (REV-01, REV-02).

The versions belong to the file. Each change of them is one undo step."""

import json

from helpers import McpClient, plant_cmd, rect_cmd, uid


def objects(cad):
    return [o["id"] for o in cad.result("objects.list")]


def test_save_compare_restore_and_undo(cad):
    bed = cad.execute(rect_cmd(0, 0, 3000, 1200))[0]
    tree = cad.execute(plant_cmd(500, 500, species="Quercus robur"))[0]
    v1 = cad.result("version.create", {"name": "Concept", "note": "first idea", "author": "Mari", "now_ms": 1000})
    assert v1["version"]["id"] == 1
    assert v1["version"]["origin"] == "agent"
    assert v1["version"]["author"] == "Mari" and v1["version"]["note"] == "first idea"
    assert v1["version"]["time_ms"] == 1000

    # Edit: move the bed, delete the tree, add a path and a layer.
    cad.execute({"op": "move_objects", "ids": [bed], "delta": [100, 0]})
    cad.execute({"op": "delete_objects", "ids": [tree]})
    path = cad.execute(rect_cmd(4000, 0, 5000, 500))[0]
    cad.execute({"op": "add_layer", "name": "Paths"})
    cad.result("version.create", {"name": "Revised", "now_ms": 2000})

    listing = cad.result("version.list")
    assert [v["name"] for v in listing["versions"]] == ["Concept", "Revised"]
    assert listing["current_is_newest"] is True
    # The second version is a small delta, not a copy.
    assert listing["versions"][0]["delta_bytes"] == 0
    assert 0 < listing["versions"][1]["delta_bytes"]

    diff = cad.result("version.diff", {"from": 1, "to": 2})
    assert diff["counts"]["objects"] == {"added": 1, "removed": 1, "changed": 1}
    assert diff["counts"]["layers"]["added"] == 1
    assert diff["truncated"] is False and diff["exact"] is True
    assert [e["id"] for e in diff["objects"]["added"]] == [path]
    assert [e["id"] for e in diff["objects"]["removed"]] == [tree]
    changed = diff["objects"]["changed"][0]
    assert changed["id"] == bed
    assert [p["path"] for p in changed["properties"]] == ["shape"]
    assert changed["properties"][0]["old"]["min"] == [0.0, 0.0]
    assert changed["properties"][0]["new"]["min"] == [100.0, 0.0]
    # Compared with the current document, nothing differs from the newest version.
    assert cad.result("version.diff", {"from": 2, "to": "current"})["totals"] == {"added": 0, "removed": 0, "changed": 0}

    # Restore the first version: the work is saved first, the content comes back in one undo step.
    before = cad.doc()
    depth_marker = cad.result("version.restore", {"id": 1, "now_ms": 3000})
    assert depth_marker["saved_version"] == {"id": 2, "new": False}  # the document equalled version 2
    assert sorted(objects(cad)) == sorted([bed, tree])
    assert cad.result("version.get", {"id": 1, "document": True})["things"]["objects"] == 2
    moved_back = next(o for o in cad.result("objects.list") if o["id"] == bed)
    assert moved_back["shape"]["min"] == [0.0, 0.0]
    assert cad.result("undo")["done"] is True
    assert cad.doc() == before

    # Edit more, restore again: now an automatic version keeps the new work.
    extra = cad.execute(rect_cmd(9000, 0, 9100, 100))[0]
    r = cad.result("version.restore", {"id": 1, "now_ms": 4000})
    assert r["saved_version"]["new"] is True
    auto = r["saved_version"]["id"]
    versions = cad.result("version.list")["versions"]
    assert versions[-1]["auto"] is True and versions[-1]["id"] == auto
    assert extra not in objects(cad)
    cad.result("version.restore", {"id": auto})
    assert extra in objects(cad)


def test_get_returns_the_document_as_it_was(cad):
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("version.create", {"name": "One"})
    cad.execute(rect_cmd(20, 0, 30, 10))
    got = cad.result("version.get", {"id": 1, "document": True})
    assert len(got["document"]["objects"]) == 1
    assert "history" not in got["document"]
    assert got["things"]["objects"] == 1
    # Without the flag there is no document.
    assert "document" not in cad.result("version.get", {"id": 1})
    # The live document is not touched, and document.get leaves the versions out.
    assert len(cad.doc()["objects"]) == 2 and "history" not in cad.doc()


def test_delete_a_version_in_the_middle_keeps_the_others(cad):
    for i in range(3):
        cad.execute(rect_cmd(i * 20, 0, i * 20 + 10, 10))
        cad.result("version.create", {"name": f"v{i + 1}"})
    assert cad.result("version.delete", {"id": 2}) == {"deleted": 2, "count": 2}
    assert [v["name"] for v in cad.result("version.list")["versions"]] == ["v1", "v3"]
    assert len(cad.result("version.get", {"id": 3, "document": True})["document"]["objects"]) == 3
    assert cad.error("version.get", {"id": 2})["data"]["kind"] == "not_found"
    cad.result("undo")
    assert cad.result("version.list")["count"] == 3


def test_versions_are_saved_in_the_file_and_open_again(make_serve, tmp_path):
    path = tmp_path / "garden.kad"
    first = make_serve()
    first.execute(plant_cmd(0, 0, species="Betula pendula"))
    first.result("version.create", {"name": "Planted", "now_ms": 7})
    first.execute(plant_cmd(2000, 0, species="Salix alba"))
    first.result("version.create", {"name": "Two trees", "now_ms": 8})
    first.result("file.save", {"path": str(path)})
    on_disk = json.loads(path.read_text())
    assert on_disk["history"]["format"] == 1
    assert len(on_disk["history"]["versions"]) == 2
    # The second version holds only what changed.
    assert len(on_disk["history"]["versions"][1]["delta"]) < len(on_disk["history"]["base"])

    second = make_serve()
    second.result("file.open", {"path": str(path)})
    listing = second.result("version.list")
    assert [v["name"] for v in listing["versions"]] == ["Planted", "Two trees"]
    assert listing["versions"][0]["time_ms"] == 7
    assert second.result("version.diff", {"from": 1, "to": 2})["totals"]["added"] == 1
    second.result("version.restore", {"id": 1})
    assert len(objects(second)) == 1


def test_a_file_without_history_opens_and_gets_one(make_serve, tmp_path):
    path = tmp_path / "plain.kad"
    writer = make_serve()
    writer.execute(rect_cmd(0, 0, 10, 10))
    writer.result("file.save", {"path": str(path)})
    assert "history" not in json.loads(path.read_text())
    reader = make_serve(path)
    assert reader.result("version.list")["count"] == 0
    reader.result("version.create", {"name": "Start"})
    assert reader.result("version.list")["current_is_newest"] is True


def test_a_hostile_history_is_dropped_or_repaired_with_notes(cad):
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("version.create", {"name": "One"})
    cad.execute(rect_cmd(20, 0, 30, 10))
    cad.result("version.create", {"name": "Two"})
    doc = json.loads(json.dumps(cad.doc()))
    file_form = {**doc, "history": None}

    # Nonsense in the field.
    file_form["history"] = "what"
    loaded = cad.result("document.load", {"document": file_form})
    assert len(loaded["notes"]) == 1
    assert cad.result("version.list")["count"] == 0

    # A damaged chain: the version that does not fit is dropped.
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("version.create", {"name": "One"})
    cad.execute(rect_cmd(20, 0, 30, 10))
    cad.result("version.create", {"name": "Two"})
    # The saved form of the document is in the file; build it from a save.
    import tempfile
    from pathlib import Path

    with tempfile.TemporaryDirectory() as d:
        p = Path(d) / "h.kad"
        cad.result("file.save", {"path": str(p)})
        saved = json.loads(p.read_text())
    saved["history"]["versions"][1]["delta"] = [
        {"op": "delete", "target": {"kind": "object", "id": uid(9999)}}
    ]
    saved["history"]["versions"][0]["name"] = "x" * 500
    loaded = cad.result("document.load", {"document": saved})
    assert loaded["notes"]
    versions = cad.result("version.list")["versions"]
    assert [v["id"] for v in versions] == [1]
    assert len(versions[0]["name"]) == 120
    # A history in an unknown format is dropped.
    saved["history"] = {"format": 99}
    assert cad.result("document.load", {"document": saved})["notes"]
    assert cad.result("version.list")["count"] == 0


def test_errors_use_the_standard_kinds(cad):
    assert cad.error("version.create", {})["code"] == -32602
    assert cad.error("version.create", {"name": "  "})["data"]["kind"] == "validation"
    assert cad.error("version.restore", {"id": 5})["data"]["kind"] == "not_found"
    assert cad.error("version.delete", {"id": 5})["data"]["kind"] == "not_found"
    assert cad.error("version.diff", {"from": 1, "to": "current"})["data"]["kind"] == "not_found"
    assert cad.error("version.diff", {"from": "x", "to": "current"})["code"] == -32602


def test_diff_cuts_long_values_and_respects_the_limit(cad):
    ids = [cad.execute(rect_cmd(i * 20, 0, i * 20 + 10, 10))[0] for i in range(5)]
    note = "n" * 400
    text = cad.execute({"op": "add_object", "shape": {"kind": "text", "at": [0, 100], "text": note, "height": 10}})[0]
    cad.result("version.create", {"name": "A"})
    cad.execute({"op": "move_objects", "ids": ids, "delta": [1, 0]})
    cad.execute({"op": "modify_object", "id": text, "shape": {"kind": "text", "at": [0, 100], "text": note + "!", "height": 10}})
    diff = cad.result("version.diff", {"from": 1, "to": "current", "limit": 3})
    assert diff["totals"]["changed"] == 6 and diff["truncated"] is True
    assert len(diff["objects"]["changed"]) == 3
    full = cad.result("version.diff", {"from": 1, "to": "current", "kinds": ["objects"]})
    assert "layers" not in full["counts"]
    text_change = next(c for c in full["objects"]["changed"] if c["id"] == text)
    old = text_change["properties"][0]["old"]
    new = text_change["properties"][0]["new"]
    assert old["cut"] and old["len"] > 160 and len(old["hash"]) == 16
    assert old["hash"] != new["hash"]


def call(cad, method, params=None):
    """Sends one request and returns (notifications before the reply, the reply)."""
    req_id = cad._next_id
    cad._next_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    cad.send(msg)
    notes = []
    while True:
        m = json.loads(cad.read_line())
        if "id" not in m:
            notes.append(m)
            continue
        return notes, m


def test_version_changes_are_change_events(cad):
    call(cad, "events.subscribe", {"kinds": ["document"]})
    notes, _ = call(cad, "version.create", {"name": "One"})
    assert len(notes) == 1
    note = notes[0]
    assert note["method"] == "events.change"
    assert note["params"]["op"] == "set_history"
    assert note["params"]["document"]["history"] is True
    assert note["params"]["origin"] == "agent"


def test_the_review_domain_in_mcp(mcp: McpClient):
    mcp.method_json("document", "execute", {"command": rect_cmd(0, 0, 10, 10)})
    created = mcp.method_json("review", "version.create", {"name": "Via MCP"})
    assert created["version"]["name"] == "Via MCP"
    assert mcp.method_json("review", "version.list")["count"] == 1
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    assert "review" in tools
    for m in ("version.create", "version.list", "version.get", "version.restore", "version.delete", "version.diff"):
        assert f"- {m}:" in tools["review"]["description"]


def test_describe_has_the_review_section(cad):
    d = cad.result("describe")
    assert "version.diff" in [m["name"] for m in d["methods"]]
    assert d["review"]["examples"]
    assert "REV-09" in d["review"]["local"]
