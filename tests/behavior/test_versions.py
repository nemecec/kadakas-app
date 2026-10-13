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
    # The older version is a small reverse delta from the newer one, not a copy; the newest is
    # stored as a change from the drawing.
    assert listing["versions"][1]["delta_bytes"] == 0
    assert 0 < listing["versions"][0]["delta_bytes"]
    assert listing["read_only"] is False

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
    deleted = cad.result("version.delete", {"id": 2})
    assert deleted["count"] == 2
    assert [(v["id"], v["name"]) for v in deleted["deleted"]] == [(2, "v2")]
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
    history = on_disk["history"]
    assert history["format"] == 2
    assert len(history["versions"]) == 2
    # The drawing is the only full copy: the newest version is a change from it, and there is no
    # separate snapshot.
    assert "snapshot" not in history["newest"] and "base" not in history
    assert len(history["newest"]["anchor"]) == 64
    # The older version is a reverse delta that is smaller than the drawing.
    assert len(history["versions"][0]["delta"]) < len(on_disk["objects"]) + 5
    assert "delta" not in history["versions"][1]

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


def test_a_save_after_more_edits_keeps_every_version(make_serve, tmp_path):
    path = tmp_path / "g.kad"
    first = make_serve()
    for i in range(3):
        first.execute(rect_cmd(i * 20, 0, i * 20 + 10, 10))
        first.result("version.create", {"name": f"v{i}"})
    first.execute(rect_cmd(500, 0, 510, 10))  # not in any version
    first.result("file.save", {"path": str(path)})
    second = make_serve(path)
    assert second.result("version.list")["read_only"] is False
    assert second.result("version.list")["current_is_newest"] is False
    for i in (1, 2, 3):
        got = second.result("version.get", {"id": i, "document": True})
        assert len(got["document"]["objects"]) == i
    assert second.result("version.diff", {"from": 3, "to": "current"})["totals"]["added"] == 1
    # Edit, save again: the versions are still all there.
    second.execute(rect_cmd(700, 0, 710, 10))
    second.result("file.save", {"path": str(path)})
    third = make_serve(path)
    assert third.result("version.list")["count"] == 3
    assert third.result("version.diff", {"from": 3, "to": "current"})["totals"]["added"] == 2


def saved_file(cad, tmp_path):
    path = tmp_path / "h.kad"
    cad.result("file.save", {"path": str(path)})
    return json.loads(path.read_text())


def test_a_history_that_cannot_be_read_stays_as_it_is_and_is_read_only(make_serve, tmp_path):
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("version.create", {"name": "One"})
    cad.execute(rect_cmd(20, 0, 30, 10))
    cad.result("version.create", {"name": "Two"})
    saved = saved_file(cad, tmp_path)
    saved["history"]["versions"][0]["delta"] = [
        {"op": "delete", "target": {"kind": "object", "id": uid(9999)}}
    ]
    original = json.loads(json.dumps(saved["history"]))
    broken = tmp_path / "broken.kad"
    broken.write_text(json.dumps(saved))

    reader = make_serve()
    opened = reader.result("file.open", {"path": str(broken)})
    # file.open reports what it could not read, like document.load.
    assert opened["ok"] is True and any("read only" in n for n in opened["notes"])
    listing = reader.result("version.list")
    assert listing["read_only"] is True and listing["count"] == 1 and listing["notes"]
    for method, params in [
        ("version.create", {"name": "x"}),
        ("version.restore", {"id": 2}),
        ("version.delete", {"id": 2}),
    ]:
        err = reader.error(method, params)
        assert err["code"] == 5 and err["data"]["kind"] == "conflict"
        assert err["data"]["notes"]
    # A save writes the history as it was.
    again = tmp_path / "again.kad"
    reader.result("file.save", {"path": str(again)})
    assert json.loads(again.read_text())["history"] == original
    # The repair needs a yes.
    assert reader.error("version.repair", {})["code"] == -32602
    kept = reader.result("version.repair", {"confirm": True})
    assert kept["kept"] == 1 and kept["notes"]
    assert reader.result("version.list")["read_only"] is False
    reader.result("version.create", {"name": "works again"})
    # Undo brings the unreadable history back.
    reader.result("undo")
    reader.result("undo")
    assert reader.result("version.list")["read_only"] is True


def test_nonsense_in_the_history_field_is_kept_and_reported(cad, tmp_path):
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("version.create", {"name": "One"})
    saved = saved_file(cad, tmp_path)
    for bad in ("what", [1, 2], {"format": 99}):
        saved["history"] = bad
        loaded = cad.result("document.load", {"document": saved})
        assert loaded["notes"]
        assert cad.result("version.list")["read_only"] is True
        out = tmp_path / "out.kad"
        cad.result("file.save", {"path": str(out)})
        assert json.loads(out.read_text())["history"] == bad


def test_the_limit_stops_the_save_and_lists_the_versions(cad):
    cad.execute(rect_cmd(0, 0, 10, 10))
    limit = cad.result("version.list")["limits"]["max_versions"]
    for i in range(limit):
        cad.result("version.create", {"name": f"v{i + 1}"})
    before = cad.result("version.list")
    err = cad.error("version.create", {"name": "one too many"})
    assert err["code"] == 6 and err["data"]["kind"] == "limit"
    data = err["data"]
    assert data["limit"] == {"kind": "versions", "max": limit, "needed": limit + 1}
    assert [v["id"] for v in data["versions"]] == list(range(1, limit + 1))
    assert {"id", "name", "bytes", "time_ms", "auto"} <= set(data["versions"][0])
    assert data["ask_the_person"] is True
    assert {o["action"] for o in data["options"]} == {"delete", "raise_limit"}
    # Nothing was saved and nothing was deleted.
    assert cad.result("version.list") == before
    # A restore is stopped the same way when it needs an automatic version.
    cad.execute(rect_cmd(50, 0, 60, 10))
    assert cad.error("version.restore", {"id": 1})["data"]["kind"] == "limit"
    # The person chooses versions to delete (not the oldest), and the save goes on.
    gone = cad.result("version.delete", {"ids": [3, 5, 7]})
    assert [v["id"] for v in gone["deleted"]] == [3, 5, 7]
    cad.result("version.create", {"name": "now it fits"})
    ids = [v["id"] for v in cad.result("version.list")["versions"]]
    assert 3 not in ids and 1 in ids and len(ids) == limit - 3 + 1


def test_delete_names_what_it_deleted_in_the_event(cad):
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("version.create", {"name": "One"})
    cad.result("version.create", {"name": "Two"})
    call(cad, "events.subscribe", {"kinds": ["document"]})
    notes, reply = call(cad, "version.delete", {"ids": [1]})
    assert reply["result"]["deleted"][0]["name"] == "One"
    assert notes[0]["params"]["op"] == "version_delete"
    assert notes[0]["params"]["detail"]["deleted"][0]["name"] == "One"


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
    assert note["params"]["op"] == "version_create"
    assert note["params"]["detail"]["created"]["name"] == "One"
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
    assert "without the person" in d["review"]["consent"]
    docs = {m["name"]: m["doc"] for m in d["methods"]}
    assert "Pass this to the person" in docs["version.create"]
    assert "version.repair" in docs
