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
    doc.versions.delete([3, 4])
    doc.versions.repair()
    doc.versions.repair(confirm=True)
    assert fake.calls == [
        ("version.list", None),
        ("version.get", {"id": 3}),
        ("version.get", {"id": 3, "document": True}),
        ("version.restore", {"id": 2}),
        ("version.delete", {"id": 2}),
        ("version.delete", {"ids": [3, 4]}),
        ("version.repair", {"confirm": False}),
        ("version.repair", {"confirm": True}),
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


class Pictures(Fake):
    """A fake that answers the render methods like the program does."""

    def request(self, method, params=None):
        self.calls.append((method, params))
        return {"png_base64": "iVBORw0KGgo=", "svg": "<svg/>"}


def test_compare_option_has_only_the_given_values():
    from kadakas.document import Versions

    assert Versions.compare_option(1) == {"from": 1, "to": "current"}
    assert Versions.compare_option(
        1, 3, mode="side_by_side", hide_unchanged=True, unchanged="hide", palette="color_blind",
        dark=False, ghosts=False, previous=True,
    ) == {
        "from": 1, "to": 3, "mode": "side_by_side", "hide_unchanged": True, "unchanged": "hide",
        "palette": "color_blind", "dark": False, "ghosts": False, "previous": True,
    }


def test_compare_renders_pass_the_option(tmp_path):
    fake = Pictures()
    doc = Document(fake)
    png = doc.versions.render_png(1, path=tmp_path / "c.png", view="top", width=100)
    assert png.startswith(b"\x89PNG") and (tmp_path / "c.png").read_bytes() == png
    assert doc.versions.render_svg(1, 2, tmp_path / "c.svg", width=500, hide_unchanged=True) == "<svg/>"
    assert (tmp_path / "c.svg").read_text() == "<svg/>"
    doc.render_svg(compare={"from": 1, "to": "current"})
    doc.render_svg()
    assert fake.calls == [
        ("render.png", {"view": "top", "width": 100, "compare": {"from": 1, "to": "current"}}),
        ("render.svg", {"width": 500, "compare": {"from": 1, "to": 2, "hide_unchanged": True}}),
        ("render.svg", {"width": 800, "compare": {"from": 1, "to": "current"}}),
        ("render.svg", {"width": 800}),
    ]
