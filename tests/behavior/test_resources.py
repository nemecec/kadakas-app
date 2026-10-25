"""Scenario: a landscape architect groups objects, makes symbols and hatches, draws with them,
and shares them between files through a library."""

import json

import ezdxf
from pytest import approx

from helpers import parse_svg, read_doc, rect_cmd, uid


def circle(cad, x, y=0, r=100):
    return cad.execute({"op": "add_object", "shape": {"kind": "circle", "center": [x, y], "radius": r}})[0]


def center(cad, oid):
    return cad.result("objects.get", {"id": oid})["shape"]["center"]


def test_groups_select_move_rotate_copy_and_undo(cad):
    a, b, c = circle(cad, 0), circle(cad, 1000), circle(cad, 5000)
    g = cad.result("group.create", {"ids": [a, b], "name": "Pair"})["id"]
    assert cad.result("selection.get")["ids"] == [g]
    # A click selects the top group. Entering the group selects members.
    assert cad.result("selection.click", {"id": b})["ids"] == [g]
    cad.result("group.enter", {"id": g})
    assert cad.result("selection.click", {"id": b})["ids"] == [b]
    assert "not inside" in cad.error("selection.click", {"id": c})["message"]
    cad.result("group.exit")
    # The group moves, turns and copies as a unit.
    cad.result("group.move", {"ids": [g], "delta": [0, 200]})
    assert center(cad, b) == [1000, 200]
    cad.result("group.rotate", {"ids": [g], "center": [0, 0], "angle": 3.141592653589793})
    assert center(cad, b)[0] == approx(-1000) and center(cad, b)[1] == approx(-200)
    copy = cad.result("group.copy", {"ids": [g], "delta": [0, 3000]})
    assert len(copy["members"]) == 2
    assert center(cad, copy["members"][1])[1] == approx(2800)
    # Every step undoes.
    for _ in range(3):
        assert cad.result("undo")["done"]
    assert center(cad, b) == [1000, 0]
    # Deleting a group deletes its members. Undo brings the whole group back.
    cad.execute({"op": "delete_objects", "ids": [g]})
    assert [o["id"] for o in cad.result("objects.list")] == [c]
    cad.result("undo")
    assert len(cad.result("objects.list")) == 4
    assert cad.result("group.ungroup", {"ids": [g]})["members"] == [a, b]
    assert cad.error("group.ungroup", {"ids": [a]})["code"] == 3


def test_nested_groups_undo_in_order(cad):
    ids = [circle(cad, x) for x in (0, 500, 1000)]
    inner = cad.result("group.create", {"ids": ids[:2]})["id"]
    outer = cad.result("group.create", {"ids": [inner, ids[2]]})["id"]
    assert cad.result("group.members", {"id": outer, "deep": True})["members"] == [ids[0], ids[1], ids[2], inner]
    assert cad.result("selection.click", {"id": ids[0]})["ids"] == [outer]
    cad.result("group.enter", {"id": outer})
    assert cad.result("selection.click", {"id": ids[0]})["ids"] == [inner]
    cad.result("group.enter", {"id": inner})
    assert cad.result("group.context")["context"] == [outer, inner]
    cad.result("group.exit")
    cad.result("group.exit")
    cad.result("undo")
    assert cad.result("objects.get", {"id": inner}).get("parent") is None
    cad.result("undo")
    assert all("parent" not in o for o in cad.result("objects.list"))


def test_symbols_follow_their_definition_and_edit_in_place(cad):
    sym = cad.result("symbol.create", {"name": "Bench", "items": [
        {"shape": {"kind": "rect", "min": [-600, -200], "max": [600, 200]}}]})["id"]
    a = cad.result("symbol.place", {"symbol": "Bench", "at": [0, 0], "rotation": 0.5, "scale": 2, "scale_y": 1})["id"]
    cad.result("symbol.place", {"symbol": sym, "at": [4000, 0]})
    assert cad.result("symbol.list")["symbols"][0]["instances"] == 2
    svg = parse_svg(cad.result("render.svg")["svg"])
    assert len(svg.findall(".//{*}polygon")) == 1 and len(svg.findall(".//{*}rect")) == 1
    # Editing the definition changes both instances.
    cad.result("resource.update", {"id": sym, "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 250}}]})
    svg = parse_svg(cad.result("render.svg")["svg"])
    # The asymmetric instance has an ellipse, which is a polygon, and the other a circle.
    assert len(svg.findall(".//{*}circle")) == 1 and len(svg.findall(".//{*}polygon")) == 1
    # Edit 2D components in place: the instance hides, the group appears, and ending applies the change.
    group = cad.result("symbol.edit", {"id": a})["group"]
    member = cad.result("group.members", {"id": group})["members"][0]
    cad.execute({"op": "modify_object", "id": member, "shape": {"kind": "circle", "center": [0, 0], "radius": 400}})
    cad.result("symbol.edit_end")
    assert cad.result("symbol.get", {"id": sym})["items"][0]["shape"]["radius"] == 400
    # Convert to group makes plain objects.
    cad.result("symbol.convert_to_group", {"ids": [a]})
    kinds = {o["shape"]["kind"] for o in cad.result("objects.list")}
    assert {"group", "symbol", "polyline", "circle"} & kinds
    # Undo reverts the conversion.
    cad.result("undo")
    assert cad.result("objects.get", {"id": a})["shape"]["kind"] == "symbol"
    # A hybrid symbol with a 3D component shows in 3D lists.
    cad.result("symbol.create", {"name": "Post", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 50}}],
                                 "solids": [{"kind": "cylinder", "center": [0, 0], "radius": 50, "height": 900}]})
    assert [s for s in cad.result("symbol.list")["symbols"] if s["name"] == "Post"][0]["kind"] == "hybrid"


def test_hatches_clip_to_the_shape_and_export_to_svg_and_dxf(cad, tmp_path):
    hatch = cad.result("resource.create", {"type": "hatch", "name": "Diag", "units": "world",
                                           "lines": [{"angle": 45, "spacing": 100}]})["id"]
    ring = [[0, 0], [3000, 0], [3000, 2000], [0, 2000]]
    r = cad.result("pattern.segments", {"pattern": "Diag", "ring": ring})
    # The lines are 100 apart, so their total length times 100 is the area.
    assert r["total_length"] * 100 == approx(6_000_000, rel=0.03)
    for a, b in r["segments"]:
        assert -1e-6 <= a[0] <= 3000 + 1e-6 and -1e-6 <= a[1] <= 2000 + 1e-6
        assert -1e-6 <= b[0] <= 3000 + 1e-6 and -1e-6 <= b[1] <= 2000 + 1e-6
    lt = cad.result("resource.create", {"type": "line_type", "name": "Dashed",
                                        "elements": [{"kind": "dash", "length": 4}, {"kind": "gap", "length": 2}]})["id"]
    cad.execute(rect_cmd(0, 0, 3000, 2000, attrs={"fill_pattern": hatch, "line_type": lt}))
    svg = cad.result("render.svg")["svg"]
    assert 'data-pattern="fill"' in svg and 'data-pattern="line_type"' in svg
    # DXF gets the hatch as a HATCH entity with its pattern lines (IOP-27), and the dashes as plain LINE entities.
    out = tmp_path / "pattern.dxf"
    cad.result("export.file", {"path": str(out), "format": "dxf"})
    doc = ezdxf.readfile(str(out))
    (h,) = [e for e in doc.modelspace() if e.dxftype() == "HATCH"]
    assert [l.angle for l in h.pattern.lines] == approx([45.0])
    lines = [e for e in doc.modelspace() if e.dxftype() == "LINE"]
    assert len(lines) > 10
    # The class default fill_pattern also works: set it on the class.
    cad.execute({"op": "modify_class", "id": uid(0), "attrs": {"fill_pattern": hatch}})
    assert cad.result("resource.get", {"id": hatch})["used_by"]["classes"] == [uid(0)]


def test_a_symbol_can_draw_a_plant_and_a_tile_repeats_a_symbol(cad):
    dot = cad.result("symbol.create", {"name": "Dot", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 20}}]})["id"]
    cad.result("resource.create", {"type": "tile", "name": "Dots", "symbol": "Dot", "width": 100, "height": 100, "units": "world"})
    t = cad.result("pattern.segments", {"pattern": "Dots", "ring": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]]})
    assert t["count"] > 100
    style = cad.result("plant_style.create", {"name": "Oak", "spread": 6000, "height": 9000, "plan_symbol": dot})["id"]
    cad.result("plants.place", {"mode": "single", "style": style, "at": [0, 0]})
    svg = cad.result("render.svg")["svg"]
    assert 'data-graphic="symbol"' in svg and "<circle" in svg
    # A tile that uses the symbol protects it from deletion.
    err = cad.error("resource.delete", {"id": dot})
    assert "used by" in err["message"]


def test_the_resource_manager_lists_filters_renames_and_purges(cad):
    h = cad.result("resource.create", {"type": "hatch", "name": "Grass", "lines": [{"spacing": 2}]})["id"]
    s = cad.result("symbol.create", {"name": "Tree", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 5},
                                                                 "attrs": {"fill_pattern": h}}]})["id"]
    cad.result("resource.create", {"type": "tile", "name": "Trees", "symbol": s, "width": 10, "height": 10})
    cad.result("resource.create", {"type": "texture", "name": "Bark", "color": [90, 60, 30, 255]})
    cad.result("plant_style.create", {"name": "Oak"})
    rows = cad.result("resource.list", {"thumbnails": True})["resources"]
    assert {r["type"] for r in rows} == {"hatch", "symbol", "tile", "texture", "plant_style"}
    assert all(r["thumbnail"].startswith("<svg") for r in rows)
    assert [r["name"] for r in cad.result("resource.list", {"search": "tre"})["resources"]] == ["Tree", "Trees"]
    assert next(r for r in rows if r["name"] == "Grass")["used_by"] == 1
    cad.result("resource.rename", {"id": "Bark", "name": "Oak bark"})
    dup = cad.result("resource.duplicate", {"id": "Oak bark"})["id"]
    assert cad.result("resource.get", {"id": dup})["name"] == "Oak bark copy"
    # The tile is unused, so the purge takes it and then the symbol and the hatch.
    dry = cad.result("resource.purge_unused", {"dry_run": True, "types": ["tile", "symbol", "hatch"]})
    assert [r["name"] for r in dry["removed"]] == ["Trees", "Tree", "Grass"]
    assert len(cad.result("resource.list")["resources"]) == 6
    cad.result("resource.purge_unused", {})
    left = {r["name"] for r in cad.result("resource.list")["resources"]}
    assert left == set()
    cad.result("undo")
    assert len(cad.result("resource.list")["resources"]) == 6


def test_libraries_import_resources_with_what_they_need(make_serve, tmp_path, monkeypatch):
    libdir = tmp_path / "library"
    libdir.mkdir()
    monkeypatch.setenv("KADAKAS_LIBRARY_DIR", str(libdir))
    src = make_serve()
    h = src.result("resource.create", {"type": "hatch", "name": "Diag", "lines": [{"angle": 45, "spacing": 2}]})["id"]
    src.result("symbol.create", {"name": "Dot", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 5}, "attrs": {"fill_pattern": h}}]})
    src.result("resource.create", {"type": "texture", "name": "Bark", "color": [1, 2, 3, 255]})
    path = src.result("resource.export_to_file", {"library": "Garden", "overwrite": True})["path"]
    assert path.endswith("Garden.kad")
    read_doc(path)
    assert src.result("library.list")["files"] == [{"file": "Garden.kad", "resources": 3}]

    dst = make_serve()
    listing = dst.result("resource.list_file", {"library": "Garden"})
    assert {r["name"] for r in listing["resources"]} == {"Diag", "Dot", "Bark"}
    r = dst.result("resource.import_from_file", {"library": "Garden", "items": [{"type": "symbol", "name": "Dot"}]})
    assert {i["name"]: i["dependency"] for i in r["imported"]} == {"Diag": True, "Dot": False}
    assert [x["name"] for x in dst.result("resource.list")["resources"]] == ["Diag", "Dot"]
    # Importing again changes nothing, because the content is the same.
    again = dst.result("resource.import_from_file", {"library": "Garden", "items": ["Dot"]})
    assert {i["status"] for i in again["imported"]} == {"reused"}
    assert len(dst.result("resource.list")["resources"]) == 2
    # One undo step takes the whole import back.
    dst.result("undo")
    assert dst.result("resource.list")["resources"] == []
    # Favorites are kept in the library folder.
    fav = {"library": "Garden.kad", "type": "symbol", "name": "Dot"}
    dst.result("library.favorite_add", fav)
    dst.result("library.favorite_add", fav)
    assert dst.result("library.favorites")["favorites"] == [fav]
    dst.result("library.favorite_remove", fav)
    assert dst.result("library.favorites")["favorites"] == []


def test_import_follows_the_path_policy(cad, tmp_path):
    outside = "/etc/hosts.kad"
    err = cad.error("resource.import_from_file", {"path": outside, "all": True})
    assert err["code"] == 4
    assert "cannot read" in err["message"] or "outside" in err["message"]
    assert cad.error("resource.import_from_file", {"library": "../x", "all": True})["code"] == -32602
    # A file that is not a Kadakas document is refused with a clear message.
    bad = tmp_path / "bad.kad"
    bad.write_text("{}", encoding="utf-8")
    assert "not a Kadakas file" in cad.error("resource.import_from_file", {"path": str(bad), "all": True})["message"]
    # An extension other than kad is refused in a restricted session.
    txt = tmp_path / "lib.txt"
    txt.write_text("x", encoding="utf-8")
    assert ".kad" in cad.error("resource.import_from_file", {"path": str(txt), "all": True})["message"]


def test_describe_lists_the_new_methods_with_examples(cad):
    d = cad.result("describe")
    names = {m["name"] for m in d["methods"]}
    for n in ["group.create", "symbol.place", "resource.import_from_file", "resource.purge_unused", "pattern.segments"]:
        assert n in names
    assert len(d["resources"]["examples"]) >= 6
