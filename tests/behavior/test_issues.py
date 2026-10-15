"""Scenario: a landscape architect and an agent review a garden plan with issues (REV-04, REV-07), and
exchange them with an engineer as a BCF file (REV-06).

The BCF files that Kadakas writes are checked against the buildingSMART XSD files of BCF 2.1 and 3.0
(`bcf_schemas/`). The check runs in `uv` with lxml (`bcf_check.py`), like the IFC test does."""

import base64
import json
import shutil
import subprocess
import zipfile
from pathlib import Path

import pytest

from helpers import McpClient, isolated_env, rect_cmd

CHECK = Path(__file__).with_name("bcf_check.py")
SCHEMAS = Path(__file__).with_name("bcf_schemas")


def validate_bcf(path):
    """The XSD errors of every XML file in a BCF archive: {name: [errors]}. Skips without uv."""
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    run = subprocess.run(
        ["uv", "run", "--no-project", "--with", "lxml", "python", "-I", str(CHECK), str(path), str(SCHEMAS)],
        capture_output=True, text=True, timeout=600, env=isolated_env(uv=True),
    )
    if run.returncode != 0 and "No solution found" in run.stderr + run.stdout:
        pytest.skip("lxml is not available")
    assert run.returncode == 0, run.stderr
    return json.loads(run.stdout.splitlines()[-1])


def garden(cad):
    bed = cad.execute(rect_cmd(0, 0, 3000, 1200))[0]
    tree = cad.execute(rect_cmd(5000, 0, 5400, 400))[0]
    return bed, tree


def test_an_agent_reviews_the_plan(cad):
    bed, tree = garden(cad)
    made = cad.result("issue.create", {
        "title": "Dead tree at the gate", "description": "The oak died in the frost.",
        "type": "error", "priority": "high", "assignee": "Mari", "labels": ["planting"],
        "due": "2026-12-01", "objects": [tree], "capture": True, "now_ms": 1_700_000_000_000,
    })
    issue_id = made["id"]
    assert made["issue"]["origin"] == "agent" and made["issue"]["status"] == "open"
    second = cad.result("issue.create", {"title": "Bed too narrow", "objects": [bed], "now_ms": 1_700_000_001_000})["id"]

    # The list filters.
    assert [i["id"] for i in cad.result("issue.list", {"assignee": "mari"})["issues"]] == [issue_id]
    assert [i["id"] for i in cad.result("issue.list", {"object": bed})["issues"]] == [second]
    assert cad.result("issue.list", {"label": "Planting"})["total"] == 1
    assert cad.result("issue.list", {"status": ["resolved"]})["total"] == 0

    # The issue with its viewpoint and its snapshot picture.
    got = cad.result("issue.get", {"id": issue_id, "snapshots": True})
    vp = got["viewpoints"][0]
    assert vp["selection"] == [tree] and vp["view"]["kind"] == "plan"
    png = base64.b64decode(vp["snapshot_base64"])
    assert png[1:4] == b"PNG"
    assert len(got["objects_ifc"][0]) == 22

    # Comments, update, resolve.
    cad.result("issue.comment", {"id": issue_id, "text": "Replace it with a field maple.", "now_ms": 1_700_000_100_000})
    cad.result("issue.update", {"id": issue_id, "status": "in_progress", "add_labels": ["urgent"]})
    resolved = cad.result("issue.resolve", {"id": issue_id, "comment": "Planted.", "now_ms": 1_700_000_200_000})
    assert resolved["issue"]["status"] == "resolved" and resolved["issue"]["comments"] == 2
    thread = cad.result("issue.get", {"id": issue_id})["comments"]
    assert [c["text"] for c in thread] == ["Replace it with a field maple.", "Planted."]
    assert all(c["origin"] == "agent" for c in thread)

    # Every call is one undo step: undo the resolve (status and comment together).
    cad.result("undo")
    assert cad.result("issue.get", {"id": issue_id})["status"] == "in_progress"
    assert len(cad.result("issue.get", {"id": issue_id})["comments"]) == 1
    cad.result("redo")
    assert cad.result("issue.get", {"id": issue_id})["status"] == "resolved"

    # The agent's own issue can be deleted; undo brings it back with its picture.
    cad.result("issue.delete", {"id": second})
    assert cad.result("issue.list")["total"] == 1
    cad.result("undo")
    assert cad.result("issue.list")["total"] == 2


def test_issues_are_review_data_and_stay_in_the_file(make_serve, tmp_path):
    cad = make_serve()
    bed, _ = garden(cad)
    issue_id = cad.result("issue.create", {"title": "Kept", "objects": [bed], "capture": True})["id"]
    cad.result("issue.comment", {"id": issue_id, "text": "A note"})
    cad.result("version.create", {"name": "V1", "now_ms": 5})
    path = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(path)})
    reopened = make_serve(path)
    again = reopened.result("issue.get", {"id": issue_id, "snapshots": True})
    assert again["title"] == "Kept" and again["comments"][0]["text"] == "A note"
    assert again["viewpoints"][0]["snapshot_base64"]
    # A restore of a version leaves the issues as they are.
    reopened.result("issue.update", {"id": issue_id, "status": "closed"})
    reopened.result("version.restore", {"id": 1, "now_ms": 9})
    assert reopened.result("issue.get", {"id": issue_id})["status"] == "closed"


def test_the_work_of_somebody_else_is_not_deleted_silently(cad):
    mine = cad.result("issue.create", {"title": "Mine", "now_ms": 1})["id"]
    theirs = cad.result("issue.create", {"title": "Theirs", "now_ms": 2})["id"]
    # A person made one of them in the app: load the document with the origin of a person.
    doc = cad.doc()
    for issue in doc["issues"]["issues"]:
        if issue["id"] == theirs:
            issue["origin"] = "user"
    cad.result("document.load", {"document": doc})
    error = cad.error("issue.delete", {"id": theirs})
    assert error["data"]["kind"] == "conflict" and error["data"]["reason"] == "confirm_needed"
    assert error["data"]["ask_the_person"] is True
    assert error["data"]["issues"][0]["id"] == theirs
    assert cad.result("issue.list")["total"] == 2
    # One issue that needs the consent stops the whole call.
    error = cad.error("issue.delete", {"ids": [mine, theirs]})
    assert error["data"]["reason"] == "confirm_needed"
    assert cad.result("issue.list")["total"] == 2
    # The agent's own issue goes; with the consent of the person the other goes too.
    assert cad.result("issue.delete", {"id": mine})["count"] == 1
    assert cad.result("issue.delete", {"id": theirs, "confirm": True})["count"] == 1
    # A comment of a person on an issue of the agent also asks.
    kept = cad.result("issue.create", {"title": "Agent issue", "now_ms": 4})["id"]
    doc = cad.doc()
    doc["issues"]["comments"] = [{"id": "00000000-0000-7000-8000-0000000000aa", "issue": kept,
                                  "author": "Mari", "origin": "user", "time": 5, "text": "Hi"}]
    cad.result("document.load", {"document": doc})
    error = cad.error("issue.delete", {"id": kept})
    assert error["data"]["reason"] == "confirm_needed"
    missing = cad.error("issue.delete", {"id": "00000000-0000-7000-8000-00000000ffff"})
    assert missing["data"]["kind"] == "not_found"


def test_events_name_the_issue_changes(make_serve):
    cad = make_serve()
    sub = cad.result("events.subscribe", {"kinds": ["issues"]})
    assert sub["kinds"] == ["issues"]
    # Over stdio the notification comes before the reply of the call that caused it.
    cad.send({"jsonrpc": "2.0", "id": 99, "method": "issue.create", "params": {"title": "Event"}})
    event = json.loads(cad.read_line())
    reply = json.loads(cad.read_line())
    assert reply["id"] == 99
    made = reply["result"]["id"]
    assert event["method"] == "events.change"
    change = event["params"]
    assert change["op"] == "add_issue" and change["origin"] == "agent"
    assert change["added"]["issues"] == [made]
    cad.send({"jsonrpc": "2.0", "id": 100, "method": "issue.comment", "params": {"id": made, "text": "Hi"}})
    event = json.loads(cad.read_line())
    reply = json.loads(cad.read_line())
    assert event["params"]["added"]["comments"] == [reply["result"]["comment"]]
    assert event["params"]["changed"]["issues"] == [made]


def test_the_mcp_review_tool_has_the_issue_methods():
    mcp = McpClient(["mcp"])
    try:
        mcp.initialize()
        tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
        text = tools["review"]["description"]
        for name in ("issue.create", "issue.list", "issue.export_bcf", "issue.import_bcf", "issue.delete"):
            assert name in text, name
        mcp.method_json("document", "execute", {"command": rect_cmd(0, 0, 100, 100)})
        made = mcp.method_json("review", "issue.create", {"title": "Via MCP", "capture": True})
        assert made["issue"]["origin"] == "agent"
        assert mcp.method_json("review", "issue.list")["total"] == 1
    finally:
        mcp.close()


def test_describe_has_the_issue_section_with_examples(cad):
    d = cad.result("describe")
    issues = d["review"]["issues"]
    assert issues["bounds"]["comments_per_issue"] == 5000
    assert issues["bounds"]["issues"] == 100000 and issues["bounds"]["issues_warning_above"] == 10000
    assert any(e["method"] == "issue.import_bcf" for e in issues["examples"])
    names = {m["name"] for m in d["methods"]}
    assert {"issue.list", "issue.get", "issue.create", "issue.update", "issue.comment", "issue.resolve",
            "issue.delete", "issue.import_bcf", "issue.export_bcf"} <= names


# ----- BCF -----


@pytest.mark.parametrize("version", ["3.0", "2.1"])
def test_bcf_export_is_valid_against_the_buildingsmart_schema(cad, tmp_path, version):
    bed, tree = garden(cad)
    cad.result("issue.create", {
        "title": "Dead tree <at> the \"gate\" & more", "description": "Line one\nLine two",
        "type": "Error", "priority": "High", "assignee": "mari@example.org", "labels": ["planting", "urgent"],
        "due": "2026-12-01", "objects": [tree, bed], "author": "ann@example.org", "capture": True,
        "now_ms": 1_700_000_000_000,
    })
    plain = cad.result("issue.create", {"title": "Plain", "author": "bo@example.org", "now_ms": 1_700_000_003_000})["id"]
    with_objects = cad.result("issue.create", {"title": "Objects only", "objects": [tree], "point": [10, 20, 0],
                                               "now_ms": 1_700_000_004_000})["id"]
    first = cad.result("issue.list")["issues"][0]["id"]
    cad.result("issue.comment", {"id": first, "text": "A comment\nwith two lines", "author": "mari@example.org",
                                 "now_ms": 1_700_000_100_000})
    cad.result("issue.comment", {"id": plain, "text": "Another", "now_ms": 1_700_000_200_000})
    path = tmp_path / f"out-{version}.bcfzip"
    out = cad.result("issue.export_bcf", {"path": str(path), "version": version})
    assert out["topics"] == 3 and out["snapshots"] == 1 and out["comments"] == 2
    assert out["viewpoints"] == 2, "one real viewpoint, one made for the issue with objects only"
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        assert "bcf.version" in names
        assert f"{first}/markup.bcf" in names
        assert any(n.endswith(".png") for n in names)
        assert len([n for n in names if n.endswith(".bcfv")]) == 2
    checked = validate_bcf(path)
    assert checked["version"] == version
    assert len(checked["files"]) >= 6
    bad = {n: e for n, e in checked["files"].items() if e}
    assert bad == {}, bad
    # A file that breaks the schema is found: the check is not a rubber stamp.
    broken = tmp_path / "broken.bcfzip"
    with zipfile.ZipFile(path) as src, zipfile.ZipFile(broken, "w") as dst:
        for name in src.namelist():
            data = src.read(name)
            if name.endswith("markup.bcf") and name.startswith(first):
                data = data.replace(b"<Title>", b"<Titel>").replace(b"</Title>", b"</Titel>")
            dst.writestr(name, data)
    checked = validate_bcf(broken)
    assert any(e for e in checked["files"].values()), "the schema should refuse a file without Title"
    assert with_objects


def test_the_ifc_global_id_of_the_export_is_the_bcf_component(cad, tmp_path):
    # An existing tree is a thing that the IFC export writes (a plain rectangle is not).
    tree = cad.execute({"op": "add_object", "shape": {"kind": "existing_tree", "at": [7000, 7000],
                                                       "species": "Betula pendula", "dbh": 400,
                                                       "spread": 6000, "height": 12000}})[0]
    made = cad.result("issue.create", {"title": "About the tree", "objects": [tree]})["id"]
    guid = cad.result("issue.get", {"id": made})["objects_ifc"][0]
    ifc = tmp_path / "model.ifc"
    cad.result("export.file", {"path": str(ifc), "format": "ifc", "overwrite": True})
    text = ifc.read_text(encoding="utf-8", errors="replace")
    assert f"'{guid}'" in text, "the GlobalId of the IFC export is the IfcGuid of the BCF component"
    bcf = tmp_path / "issues.bcfzip"
    cad.result("issue.export_bcf", {"path": str(bcf)})
    with zipfile.ZipFile(bcf) as z:
        bcfv = [n for n in z.namelist() if n.endswith(".bcfv")]
        assert bcfv
        assert f'IfcGuid="{guid}"' in z.read(bcfv[0]).decode()


def test_bcf_round_trip_between_two_documents(make_serve, tmp_path):
    a = make_serve()
    _, tree = garden(a)
    made = a.result("issue.create", {"title": "Dead tree", "objects": [tree], "capture": True,
                                     "assignee": "Mari", "labels": ["planting"], "now_ms": 1_700_000_000_000})["id"]
    a.result("issue.comment", {"id": made, "text": "Please check", "now_ms": 1_700_000_100_000})
    path = tmp_path / "out.bcfzip"
    a.result("issue.export_bcf", {"path": str(path)})

    b = make_serve()
    garden(b)
    result = b.result("issue.import_bcf", {"path": str(path)})
    assert result["imported"] == 1 and result["created"] == [made] and result["pictures"] == 1
    got = b.result("issue.get", {"id": made, "snapshots": True})
    assert got["title"] == "Dead tree" and got["assignee"] == "Mari"
    assert got["comments"][0]["text"] == "Please check"
    assert got["viewpoints"][0]["snapshot_base64"]
    # The same file again changes nothing.
    again = b.result("issue.import_bcf", {"path": str(path)})
    assert again["imported"] == 0 and again["unchanged"] == 1

    # The engineer changes the issue and sends it back: the architect is asked, never overwritten.
    b.result("issue.update", {"id": made, "status": "resolved", "now_ms": 1_700_000_500_000})
    b.result("issue.comment", {"id": made, "text": "Fixed", "now_ms": 1_700_000_600_000})
    back = tmp_path / "back.bcfzip"
    b.result("issue.export_bcf", {"path": str(back)})
    error = a.error("issue.import_bcf", {"path": str(back)})
    assert error["data"]["kind"] == "conflict" and error["data"]["reason"] == "bcf_conflict"
    assert error["data"]["ask_the_person"] is True
    assert error["data"]["conflicts"][0]["id"] == made
    assert a.result("issue.get", {"id": made})["status"] == "open"
    plan = a.result("issue.import_bcf", {"path": str(back), "dry_run": True})
    assert len(plan["plan"]["conflicts"]) == 1
    taken = a.result("issue.import_bcf", {"path": str(back), "on_conflict": "replace"})
    assert taken["replaced"] == 1
    assert a.result("issue.get", {"id": made})["status"] == "resolved"
    assert [c["text"] for c in a.result("issue.get", {"id": made})["comments"]] == ["Please check", "Fixed"]
    # One undo step takes the import back.
    a.result("undo")
    assert a.result("issue.get", {"id": made})["status"] == "open"


def hostile(path, entries):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in entries:
            z.writestr(name, data)


def test_hostile_bcf_archives_are_refused_or_listed(cad, tmp_path):
    version = ("bcf.version", '<Version VersionId="3.0"/>')
    good = ("aaaaaaaa-1111-4222-8333-444455556666/markup.bcf",
            '<Markup><Topic Guid="aaaaaaaa-1111-4222-8333-444455556666" TopicType="Issue" TopicStatus="Open">'
            "<Title>Good</Title><CreationDate>2026-01-01T00:00:00Z</CreationDate>"
            "<CreationAuthor>a</CreationAuthor></Topic></Markup>")
    cases = {
        "traversal": ([version, ("../../evil.txt", "x")], "validation"),
        "absolute": ([version, ("/etc/cron.d/x", "x")], "validation"),
        "bomb": ([version, ("a/bomb.bin", b"\0" * (8 * 1024 * 1024))], "limit"),
        "not_bcf": ([("readme.txt", "hi")], "validation"),
        "future_version": ([("bcf.version", '<Version VersionId="9.9"/>')], "validation"),
    }
    for name, (entries, kind) in cases.items():
        path = tmp_path / f"{name}.bcfzip"
        hostile(path, entries)
        error = cad.error("issue.import_bcf", {"path": str(path)})
        assert error["data"]["kind"] == kind, (name, error)
    garbage = tmp_path / "garbage.bcfzip"
    garbage.write_bytes(b"PK\x03\x04 this is not a real archive")
    assert cad.error("issue.import_bcf", {"path": str(garbage)})["data"]["kind"] == "validation"
    # A DTD with entities is refused for the topic that has it, and the good topic comes in.
    bomb = ("bbbbbbbb-1111-4222-8333-444455556666/markup.bcf",
            '<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "aaaa"><!ENTITY b "&a;&a;&a;&a;">]>'
            "<Markup><Topic Guid=\"bbbbbbbb-1111-4222-8333-444455556666\"><Title>&b;</Title></Topic></Markup>")
    path = tmp_path / "mixed.bcfzip"
    hostile(path, [version, good, bomb])
    result = cad.result("issue.import_bcf", {"path": str(path), "now_ms": 5})
    assert result["imported"] == 1
    assert [s["topic"] for s in result["topics_skipped"]] == ["bbbbbbbb-1111-4222-8333-444455556666"]
    assert cad.result("issue.list")["total"] == 1


def test_a_path_outside_the_allowed_folders_is_refused(cad):
    error = cad.error("issue.export_bcf", {"path": "/etc/kadakas-test.bcfzip"})
    assert error["data"]["kind"] == "policy"
    error = cad.error("issue.import_bcf", {"path": "/etc/hosts"})
    assert error["data"]["kind"] == "policy"


CASES = Path(__file__).with_name("bcf_cases")


def test_the_official_buildingsmart_test_files_import(cad, tmp_path):
    """Real files from the BCF-XML repository of buildingSMART (BCF 2.1): the reader takes them."""
    minimal = cad.result("issue.import_bcf", {"path": str(CASES / "MinimumInformation.bcf"), "now_ms": 5})
    assert minimal["imported"] == 1 and minimal["topics_skipped"] == []
    ortho = cad.result("issue.import_bcf", {"path": str(CASES / "OrthogonalCamera.bcf"), "now_ms": 5})
    assert ortho["imported"] == 1 and ortho["pictures"] >= 1
    full = cad.result("issue.import_bcf", {"path": str(CASES / "MaximumInformation.bcf"), "now_ms": 5})
    assert full["imported"] == 2 and full["topics_skipped"] == [] and full["version"] == "2.1"
    # What the reader does not take is counted, not hidden.
    text = " ".join(full["warnings"])
    for word in ("BIM snippets", "coloring", "clipping planes", "related topics"):
        assert word in text, text
    issues = {i["title"]: i for i in cad.result("issue.list", {"origin": "agent"})["issues"]}
    big = issues["Maximum Content"]
    assert big["status"] == "open" and big["priority"] == "High" and big["comments"] == 4
    assert big["viewpoints"] == 3 and big["assignee"] == "linhard@iabi.eu"
    assert big["labels"] == ["Structural", "IT Development"] and big["due"] == "2016-10-02"
    # The same file again is the same content: no conflict.
    again = cad.result("issue.import_bcf", {"path": str(CASES / "MaximumInformation.bcf")})
    assert again["imported"] == 0 and again["unchanged"] == 2
    # What came in goes out as a file that the schema accepts.
    ours = tmp_path / "out.bcfzip"
    cad.result("issue.export_bcf", {"path": str(ours), "version": "3.0"})
    checked = validate_bcf(ours)
    bad = {n: e for n, e in checked["files"].items() if e}
    assert bad == {}, bad
