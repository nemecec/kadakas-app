"""``doc.versions``: the wrapper around ``version.*``. A fake transport stands in for the program."""

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


def test_create_passes_only_the_given_values():
    fake = Fake()
    doc = Document(fake)
    doc.versions.create("Concept")
    doc.versions.create("Final", "for print", author="Mari", now_ms=5)
    assert fake.calls == [
        ("version.create", {"name": "Concept"}),
        ("version.create", {"name": "Final", "note": "for print", "author": "Mari", "now_ms": 5}),
    ]


def test_read_and_change_calls():
    fake = Fake()
    doc = Document(fake)
    doc.versions.list()
    doc.versions.get(3)
    doc.versions.get(3, document=True)
    doc.versions.restore(2)
    doc.versions.delete(2)
    assert fake.calls == [
        ("version.list", None),
        ("version.get", {"id": 3}),
        ("version.get", {"id": 3, "document": True}),
        ("version.restore", {"id": 2}),
        ("version.delete", {"id": 2}),
    ]


def test_diff_names_the_sides_from_and_to():
    fake = Fake()
    doc = Document(fake)
    doc.versions.diff(1)
    doc.versions.diff(1, 2, kinds=["objects"], limit=10)
    assert fake.calls == [
        ("version.diff", {"from": 1, "to": "current"}),
        ("version.diff", {"from": 1, "to": 2, "kinds": ["objects"], "limit": 10}),
    ]
