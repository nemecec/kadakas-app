"""``doc.update_from_dxf`` and ``doc.compare_drawings``: the wrappers around ``import.update``,
``import.apply`` and ``dxf.compare``. A fake transport stands in for the program."""

from __future__ import annotations

import base64

from kadakas.document import Document


class Fake:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict | None]] = []

    def request(self, method, params=None):
        self.calls.append((method, params))
        if method == "import.update":
            return {"update_id": "u1", "changes": []}
        if method == "import.apply":
            return {"applied": {"added": 0}}
        return {"image": {"png_base64": base64.b64encode(b"PNG").decode()}}

    def close(self) -> None:
        pass


def test_update_without_accept_only_plans():
    fake = Fake()
    doc = Document(fake)
    r = doc.update_from_dxf("site_v2.dxf", drawing="site.dxf", layers=["WALLS"])
    assert r["update_id"] == "u1"
    assert fake.calls == [("import.update", {"path": "site_v2.dxf", "drawing": "site.dxf", "layers": ["WALLS"]})]


def test_update_with_accept_applies_the_plan():
    fake = Fake()
    doc = Document(fake)
    r = doc.update_from_dxf("site_v2.dxf", accept="all", reject=["c1"], conflicts="overwrite")
    assert r["applied"] == {"added": 0} and r["update"]["update_id"] == "u1"
    assert fake.calls[1] == (
        "import.apply",
        {"update_id": "u1", "accept": "all", "reject": ["c1"], "conflicts": "overwrite"},
    )


def test_compare_passes_the_options_and_writes_the_picture(tmp_path):
    fake = Fake()
    doc = Document(fake)
    r = doc.compare_drawings("a.dxf", "b.dxf", render={"format": "png", "revision_clouds": True}, tolerance=3)
    assert fake.calls == [
        (
            "dxf.compare",
            {
                "base": "a.dxf",
                "new": "b.dxf",
                "options": {"tolerance": 3},
                "render": {"format": "png", "revision_clouds": True},
            },
        )
    ]
    out = tmp_path / "compare.png"
    Document.save_compare_image(r, out)
    assert out.read_bytes() == b"PNG"
    svg = tmp_path / "c.svg"
    Document.save_compare_image({"image": {"svg": "<svg/>"}}, svg)
    assert svg.read_text() == "<svg/>"
