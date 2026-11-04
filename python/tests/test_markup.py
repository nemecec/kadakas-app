"""``doc.markups``: the wrapper around ``markup.*``. A fake transport stands in for the program."""

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
    marks = [{"kind": "arrow", "points": [[0.1, 0.1], [0.5, 0.5]]}]
    doc.markups.create(marks, note="Hedge here", issue="i1")
    doc.markups.list(origin="user")
    doc.markups.get("m1")
    doc.markups.get("m1", render=False, width=400, samples=10, pick=False)
    doc.markups.render("m1")
    doc.markups.render("m1", with_marks=False, width=300)
    assert fake.calls == [
        ("markup.create", {"marks": marks, "note": "Hedge here", "issue": "i1"}),
        ("markup.list", {"origin": "user"}),
        ("markup.get", {"id": "m1"}),
        ("markup.get", {"id": "m1", "render": False, "width": 400, "samples": 10, "pick": False}),
        ("markup.render", {"id": "m1"}),
        ("markup.render", {"id": "m1", "with_marks": False, "width": 300}),
    ]


def test_change_calls():
    fake = Fake()
    doc = Document(fake)
    doc.markups.update("m1", issue=None, name="Where")
    doc.markups.clear("m1")
    doc.markups.clear("m1", confirm=True)
    doc.markups.delete("m1")
    doc.markups.delete(["m1", "m2"], confirm=True)
    assert fake.calls == [
        ("markup.update", {"id": "m1", "issue": None, "name": "Where"}),
        ("markup.clear", {"id": "m1"}),
        ("markup.clear", {"id": "m1", "confirm": True}),
        ("markup.delete", {"id": "m1"}),
        ("markup.delete", {"ids": ["m1", "m2"], "confirm": True}),
    ]
