"""Scenario: a person inks a comment on the live view and an agent reads it (REV-11, stage 1).

The person's markup is loaded into the session with the origin `user`, as the app saves it. The
agent then reads the marks, the pictures and what is under each mark, and files the markup with an
issue, which puts it into the BCF file."""

import base64
import io
import zipfile

from helpers import McpClient, rect_cmd

PNG = b"\x89PNG\r\n\x1a\n"

PLAN_VIEW = {
    "view": {"kind": "plan", "center": [500, 500], "zoom": 0.2},
    "canvas": [400, 300],
}


def filled_rect(cad):
    cmd = rect_cmd(0, 0, 1000, 1000)
    cmd["attrs"] = {"fill": [200, 200, 200, 255]}
    return cad.execute(cmd)[0]


def person_markup(cad, marks=None, **extra):
    """Puts a markup of a person into the document and gives its ID."""
    markup = {
        "id": "00000000-0000-7000-8000-0000000000a1",
        "author": "Mari", "origin": "user", "time": 1_700_000_000_000,
        "note": "Add a hedge along the fence.",
        "viewpoint": PLAN_VIEW,
        "marks": marks or [
            {"id": 1, "kind": "arrow", "color": "#d62828", "width": 4,
             "points": [[0.85, 0.2], [0.5, 0.5]]},
            {"id": 2, "kind": "note", "color": "#1d4ed8", "width": 3,
             "points": [[0.85, 0.15]], "text": "Hedge here"},
        ],
        **extra,
    }
    doc = cad.doc()
    doc["markups"] = [markup]
    cad.result("document.load", {"document": doc})
    return markup["id"]


def test_the_agent_sees_what_the_person_drew(cad):
    rect = filled_rect(cad)
    markup_id = person_markup(cad)
    listed = cad.result("markup.list")
    assert listed["total"] == 1 and listed["markups"][0]["origin"] == "user"
    got = cad.result("markup.get", {"id": markup_id, "width": 400})
    arrow = got["marks"][0]
    assert arrow["kind"] == "arrow" and arrow["color"] == "#d62828"
    assert arrow["points_canvas"] == [[340.0, 60.0], [200.0, 150.0]]
    assert arrow["bbox"]["relative"] == [0.5, 0.2, 0.85, 0.5]
    # The middle of the view is the middle of the rectangle: the tip is on it, the tail is not.
    assert arrow["under"]["tip"]["ids"] == [rect] and arrow["under"]["tail"]["ids"] == []
    assert arrow["hint3d"]["at"] == "tip" and arrow["hint3d"]["point"] == [500.0, 500.0]
    assert got["marks"][1]["text"] == "Hedge here"
    assert got["viewpoint"]["mode"] == "plan" and got["note"].startswith("Add a hedge")
    # Two pictures: the model without the marks, and with them.
    clean = base64.b64decode(got["clean"]["png_base64"])
    marked = base64.b64decode(got["with_marks"]["png_base64"])
    assert clean.startswith(PNG) and marked.startswith(PNG) and clean != marked
    assert (got["clean"]["width"], got["clean"]["height"]) == (400, 300)
    one = cad.result("markup.render", {"id": markup_id, "with_marks": False, "width": 400})
    assert base64.b64decode(one["png_base64"]) == clean
    # The markup is no model: not an object, and not in the object list.
    assert all(o["id"] == rect for o in cad.doc()["objects"])


def test_an_agent_draws_a_markup_in_one_undo_step(cad):
    filled_rect(cad)
    made = cad.result("markup.create", {
        "viewpoint": PLAN_VIEW, "note": "Check the edge.", "now_ms": 5,
        "marks": [{"kind": "rectangle", "points": [[0.3, 0.3], [0.7, 0.7]]},
                  {"kind": "pen", "points_canvas": [[10, 10], [100, 20]], "color": "#2a9d8f"}],
    })
    assert made["marks"] == 2 and made["markup"]["origin"] == "agent"
    assert cad.result("markup.get", {"id": made["id"], "render": False})["marks"][1]["points"] == \
        [[0.025, 0.03333], [0.25, 0.06667]]
    cad.result("undo")
    assert cad.result("markup.list")["total"] == 0
    cad.result("redo")
    assert cad.result("markup.list")["total"] == 1


def test_the_work_of_a_person_is_not_deleted_silently(cad):
    markup_id = person_markup(cad)
    for method in ("markup.delete", "markup.clear"):
        error = cad.error(method, {"id": markup_id})
        assert error["data"]["reason"] == "confirm_needed" and error["data"]["ask_the_person"] is True
    assert cad.result("markup.list")["total"] == 1
    assert cad.result("markup.clear", {"id": markup_id, "confirm": True})["removed"] == 2
    assert cad.result("markup.delete", {"id": markup_id, "confirm": True})["count"] == 1


def test_limits_are_refused_and_nothing_is_made(cad):
    too_many = [{"kind": "pen", "points": [[0.1, 0.1]]}] * 201
    assert cad.error("markup.create", {"marks": too_many})["data"]["kind"] == "validation"
    assert cad.error("markup.create", {"marks": [{"kind": "arrow", "points": [[0, 0]]}]})
    assert cad.result("markup.list")["total"] == 0


def test_a_restore_of_a_version_leaves_the_markups_and_the_file_keeps_them(cad, tmp_path):
    filled_rect(cad)
    markup_id = person_markup(cad)
    cad.result("version.create", {"name": "A", "now_ms": 1})
    cad.execute(rect_cmd(2000, 0, 3000, 1000))
    cad.result("version.restore", {"id": 1, "now_ms": 2})
    assert cad.result("markup.list")["total"] == 1
    f = str(tmp_path / "ink.kad")
    cad.result("file.save", {"path": f})
    cad.result("file.open", {"path": f})
    assert cad.result("markup.get", {"id": markup_id, "render": False})["marks"][0]["kind"] == "arrow"


def test_unknown_data_of_a_newer_file_stays(cad):
    markup_id = person_markup(
        cad,
        marks=[{"id": 1, "kind": "lasso", "points": [[0.1, 0.1]], "pressure": [1, 2]},
               {"id": 2, "kind": "pen", "points": [[0.2, 0.2]]}],
        future_field={"a": 1},
    )
    doc = cad.doc()
    assert doc["markups"][0]["future_field"] == {"a": 1}
    assert doc["markups"][0]["marks"][0]["kind"] == "lasso"
    assert doc["markups"][0]["marks"][0]["pressure"] == [1, 2]
    got = cad.result("markup.get", {"id": markup_id, "render": False})
    assert got["marks"][0]["known"] is False and "under" not in got["marks"][0]
    assert any("lasso" in n for n in got["notes"])


def test_a_markup_of_an_issue_goes_into_the_bcf_snapshot(cad):
    filled_rect(cad)
    markup_id = person_markup(cad)
    issue = cad.result("issue.create", {"title": "Hedge", "now_ms": 1})["id"]
    cad.result("markup.update", {"id": markup_id, "issue": issue})
    out = cad.result("issue.export_bcf", {})
    assert out["viewpoints"] == 1 and out["snapshots"] == 1 and out["markups"]["in_file"] == 1
    z = zipfile.ZipFile(io.BytesIO(base64.b64decode(out["base64"])))
    pictures = [n for n in z.namelist() if n.endswith(".png")]
    assert len(pictures) == 1 and z.read(pictures[0]).startswith(PNG)
    # A second session keeps the picture as it is and reads no marks from it.
    from helpers import JsonRpc
    other = JsonRpc(["serve"])
    try:
        assert other.result("issue.import_bcf", {"base64": out["base64"]})["imported"] == 1
        assert other.result("markup.list")["total"] == 0
        assert other.result("issue.get", {"id": issue, "snapshots": True})["viewpoints"][0]["snapshot_base64"]
    finally:
        other.close()


def test_the_mcp_review_tool_has_the_markup_methods(cad):
    d = cad.result("describe")
    assert d["review"]["markup"]["bounds"]["marks_per_markup"] == 200
    names = {m["name"] for m in d["methods"]}
    assert {"markup.list", "markup.get", "markup.create", "markup.update", "markup.delete",
            "markup.clear", "markup.render"} <= names
    mcp = McpClient(["mcp"])
    try:
        mcp.initialize()
        tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
        for name in ("markup.create", "markup.get", "markup.render"):
            assert name in tools["review"]["description"], name
        made = mcp.method_json("review", "markup.create", {
            "viewpoint": PLAN_VIEW, "marks": [{"kind": "pen", "points": [[0.1, 0.1], [0.2, 0.2]]}]})
        assert made["markup"]["origin"] == "agent"
        assert mcp.method_json("review", "markup.list")["total"] == 1
    finally:
        mcp.close()
