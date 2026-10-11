"""Scenario: a landscape architect gives plant styles pictures, saves the drawing and opens it again.

The test pictures are made in code. No picture file is in the repository."""

import base64
import struct
import zlib

import pytest


def png_bytes(width, height, rgb=(30, 140, 60)):
    """A small PNG made with the standard library: a solid color with a bright first row."""
    rows = b""
    for y in range(height):
        color = (250, 250, 20) if y == 0 else rgb
        rows += b"\x00" + bytes(color) * width

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )


def make_style(cad, name="Field maple"):
    return cad.result("plant_style.create", {"name": name, "category": "tree", "spread": 7000})["id"]


def test_set_get_clear_and_undo(cad):
    sid = make_style(cad)
    assert cad.result("plant_style.get_picture", {"id": sid}) == {"id": sid, "has_picture": False}
    png = png_bytes(48, 24)
    r = cad.result("plant_style.set_picture", {"id": sid, "base64": base64.b64encode(png).decode(), "name": "maple.png"})
    assert r["created"] is True
    assert (r["picture"]["width"], r["picture"]["height"], r["picture"]["format"]) == (48, 24, "png")
    got = cad.result("plant_style.get_picture", {"id": sid, "bytes": True, "thumbnail": True})
    assert base64.b64decode(got["base64"]) == png
    assert base64.b64decode(got["thumbnail_base64"]).startswith(b"\x89PNG")
    assert cad.result("plant_style.get", {"id": sid})["picture"]["sha256"] == r["picture"]["sha256"]
    assert [x["name"] for x in cad.result("resource.list", {"type": "image"})["resources"]] == [
        "Picture " + r["picture"]["sha256"][:8]
    ]
    cad.result("plant_style.clear_picture", {"id": sid})
    assert cad.result("plant_style.get_picture", {"id": sid})["has_picture"] is False
    cad.result("undo")
    assert cad.result("plant_style.get_picture", {"id": sid})["has_picture"] is True


def test_the_picture_is_saved_in_the_file_and_shared_between_styles(make_serve, tmp_path):
    path = tmp_path / "garden.kad"
    first = make_serve()
    a, b = make_style(first, "A"), make_style(first, "B")
    data = base64.b64encode(png_bytes(16, 16)).decode()
    first.result("plant_style.set_picture", {"id": a, "base64": data})
    second = first.result("plant_style.set_picture", {"id": b, "base64": data})
    assert second["created"] is False
    assert len(first.result("resource.list", {"type": "image"})["resources"]) == 1
    first.result("file.save", {"path": str(path)})
    reopened = make_serve()
    reopened.result("file.open", {"path": str(path)})
    for name in ("A", "B"):
        assert reopened.result("plant_style.get_picture", {"id": name})["has_picture"] is True


def test_pictures_from_files_and_csv(cad, tmp_path):
    (tmp_path / "pics").mkdir()
    (tmp_path / "pics" / "maple.png").write_bytes(png_bytes(20, 20))
    sid = make_style(cad)
    cad.result("plant_style.set_picture", {"id": sid, "path": str(tmp_path / "pics" / "maple.png")})
    assert cad.result("plant_style.get_picture", {"id": sid})["picture"]["source"] == "maple.png"
    csv = "name,spread,image\nBirch,4,pics/maple.png\nOak,5,../x.png\n"
    r = cad.result("plant_style.import_csv", {"csv": csv, "unit": "m", "pictures_dir": str(tmp_path)})
    assert r["pictures"] == 1 and len(r["warnings"]) == 1
    assert cad.result("plant_style.get_picture", {"id": "Birch"})["has_picture"] is True
    # Birch uses the same file as the first style: still one picture in the document.
    assert len(cad.result("resource.list", {"type": "image"})["resources"]) == 1


@pytest.mark.parametrize(
    "payload",
    [
        b"not a picture",
        b"\x89PNG\r\n\x1a\n" + b"\x00" * 40,
        png_bytes(40, 40)[:60],
        b"\xff\xd8\xff" + b"\x00" * 30,
    ],
)
def test_hostile_files_are_refused_and_the_session_goes_on(cad, payload):
    sid = make_style(cad)
    err = cad.error("plant_style.set_picture", {"id": sid, "base64": base64.b64encode(payload).decode()})
    assert err["data"]["kind"] in ("validation", "invalid_params")
    assert cad.result("plant_style.get_picture", {"id": sid})["has_picture"] is False


def test_a_picture_with_too_many_pixels_is_refused(cad):
    sid = make_style(cad)
    # A PNG header that claims 9000 x 9000 pixels (81 million) in a tiny file.
    png = bytearray(png_bytes(8, 8))
    png[16:24] = struct.pack(">II", 9000, 9000)
    err = cad.error("plant_style.set_picture", {"id": sid, "base64": base64.b64encode(bytes(png)).decode()})
    assert err["data"]["kind"] == "validation"


def test_the_picture_methods_are_in_describe(cad):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    assert {"plant_style.set_picture", "plant_style.clear_picture", "plant_style.get_picture"} <= names
    assert d["plant_pictures"]["formats"] == ["png", "jpeg"]


def test_an_image_column_prints_the_picture_in_pdf_and_svg_and_csv_skips_it(cad, tmp_path):
    sid = make_style(cad, "Field maple")
    make_style(cad, "Silver birch")
    cad.result("plants.place", {"mode": "poly", "style": "Field maple", "points": [[0, 0]]})
    cad.result("plants.place", {"mode": "poly", "style": "Silver birch", "points": [[1000, 0]]})
    cad.result("plant_style.set_picture", {"id": sid, "base64": base64.b64encode(png_bytes(40, 20)).decode()})
    db = {
        "source": "plant_list",
        "columns": [{"header": "Style", "expr": "Style"}, {"header": "Picture", "expr": "Image"}, {"header": "Count", "expr": "Count"}],
    }
    ws = cad.result("worksheet.create", {"name": "Plants with pictures", "databases": [db]})["id"]
    sheet = cad.result("sheet.create", {"name": "L-1", "paper": "a3"})["layer"]
    cad.result("worksheet.place", {"id": ws, "at": [20, 280], "layer": sheet, "text_height": 3})
    # The sheet SVG embeds one picture, for the style that has one.
    svg = cad.result("sheet.render", {"sheet": sheet})["svg"]
    assert svg.count("<image") == 1
    pdf = tmp_path / "t.pdf"
    cad.result("export.file", {"format": "pdf", "path": str(pdf)})
    data = pdf.read_bytes()
    assert b"/Subtype /Image" in data or b"/Subtype/Image" in data
    # CSV cannot carry pictures: the cell is empty, the other cells stay.
    csv_text = cad.result("worksheet.export", {"id": ws, "format": "csv"})["csv"]
    lines = csv_text.strip().splitlines()
    assert lines[0].startswith("Style,Picture,Count")
    assert any(line.startswith("Field maple,,1") for line in lines)
    # The cell value is the picture resource.
    values = cad.result("worksheet.evaluate", {"id": ws})["values"]
    images = [row[1] for row in values if isinstance(row[1], dict)]
    assert len(images) == 1 and images[0]["image"] == cad.result("plant_style.get", {"id": sid})["image"]
