"""``doc.issues``: the wrapper around ``issue.*``. A fake transport stands in for the program."""

from __future__ import annotations

from kadakas.document import Document


class Fake:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict | None]] = []

    def request(self, method, params=None):
        self.calls.append((method, params))
        return {"ok": True}

    def close(self) -> None:
        pass


def test_create_and_read_calls_pass_only_the_given_values():
    fake = Fake()
    doc = Document(fake)
    doc.issues.create("Dead tree", objects=["a"], capture=True)
    doc.issues.list(status=["open"], assignee="Mari")
    doc.issues.get("id1")
    doc.issues.get("id1", snapshots=True)
    assert fake.calls == [
        ("issue.create", {"title": "Dead tree", "objects": ["a"], "capture": True}),
        ("issue.list", {"status": ["open"], "assignee": "Mari"}),
        ("issue.get", {"id": "id1"}),
        ("issue.get", {"id": "id1", "snapshots": True}),
    ]


def test_change_calls():
    fake = Fake()
    doc = Document(fake)
    doc.issues.update("id1", assignee=None, add_labels=["x"])
    doc.issues.comment("id1", "Hello", viewpoint="vp")
    doc.issues.resolve("id1")
    doc.issues.resolve("id1", "Done", status="closed")
    assert fake.calls == [
        ("issue.update", {"id": "id1", "assignee": None, "add_labels": ["x"]}),
        ("issue.comment", {"id": "id1", "text": "Hello", "viewpoint": "vp"}),
        ("issue.resolve", {"id": "id1"}),
        ("issue.resolve", {"id": "id1", "comment": "Done", "status": "closed"}),
    ]


def test_delete_needs_confirm_to_be_sent():
    fake = Fake()
    doc = Document(fake)
    doc.issues.delete("id1")
    doc.issues.delete(["id1", "id2"])
    doc.issues.delete("id1", confirm=True)
    assert fake.calls == [
        ("issue.delete", {"id": "id1"}),
        ("issue.delete", {"ids": ["id1", "id2"]}),
        ("issue.delete", {"id": "id1", "confirm": True}),
    ]


def test_bcf_calls(tmp_path):
    fake = Fake()
    doc = Document(fake)
    doc.issues.export_bcf()
    doc.issues.export_bcf(tmp_path / "a.bcfzip", version="2.1", open_only=True)
    doc.issues.import_bcf(tmp_path / "a.bcfzip")
    doc.issues.import_bcf(base64="AA==", on_conflict="skip", dry_run=True)
    assert fake.calls == [
        ("issue.export_bcf", {"version": "3.0"}),
        ("issue.export_bcf", {"version": "2.1", "path": str(tmp_path / "a.bcfzip"), "open_only": True}),
        ("issue.import_bcf", {"path": str(tmp_path / "a.bcfzip")}),
        ("issue.import_bcf", {"base64": "AA==", "on_conflict": "skip", "dry_run": True}),
    ]
