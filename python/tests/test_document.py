"""The high-level document API against a real ``kadakas serve``."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import kadakas
from kadakas import KadakasError


def test_add_shapes_and_read_them_back(doc):
    line = doc.add.line([0, 0], [5000, 0])
    circle = doc.add.circle([100, 200], 300)
    rect = doc.add.rect([0, 0], [10, 20])
    arc = doc.add.arc([0, 0], 100, 0, 1.5)
    poly = doc.add.polyline([[0, 0], [10, 0], [10, 10]], closed=True)
    text = doc.add.text([5, 5], "hello", height=100)
    assert [o.kind for o in (line, circle, rect, arc, poly, text)] == [
        "line", "circle", "rect", "arc", "polyline", "text"]
    assert circle.radius == 300 and circle["center"] == [100.0, 200.0]
    assert text.text == "hello"
    with pytest.raises(AttributeError):
        circle.nothing  # noqa: B018
    assert doc.summary()["object_count"] == 6
    assert "circle" in repr(doc) and "Obj(" in repr(circle)


def test_curved_polyline_vertices(doc):
    p = doc.add.polyline([[0, 0], {"p": [3000, 0], "kind": "arc", "radius": 400}, [3000, 2000]])
    assert p.kind == "polyline"


def test_layers_and_classes(doc):
    layer = doc.add_layer("Planting", elevation=250)
    cls = doc.add_class("Trees", pen=[0, 128, 0, 255])
    assert layer.name == "Planting" and layer.elevation == 250
    assert cls.name == "Trees"
    layer.activate()
    cls.activate()
    obj = doc.add.circle([0, 0], 5)
    assert obj.layer.name == "Planting" and obj.class_.name == "Trees"
    other = doc.add.circle([0, 0], 5, layer=doc.layers[0], class_=doc.classes[0])
    assert other.layer_id == doc.layers[0].id
    assert [o.id for o in doc.objects(layer="Planting")] == [obj.id]
    assert [o.id for o in layer.objects()] == [obj.id]
    assert [o.id for o in doc.objects(class_="Trees")] == [obj.id]
    assert doc.active_layer.id == layer.id
    layer.modify(visible=False)
    assert not doc.layer("Planting").visible
    cls.modify(name="Big trees")
    assert doc.class_("Big trees").id == cls.id
    assert doc.layer(layer).id == layer.id and doc.class_(cls).id == cls.id
    with pytest.raises(KeyError):
        doc.layer("nothing")
    with pytest.raises(KeyError):
        doc.class_("nothing")
    assert "Layer(" in repr(layer) and "Class(" in repr(cls)


def test_queries(doc):
    doc.add.circle([0, 0], 100)
    doc.add.circle([0, 0], 500)
    doc.add.line([0, 0], [1, 1])
    assert len(doc.objects(kind="circle")) == 2
    big = doc.query(kind="circle", where=lambda o: o.radius > 200)
    assert [o.radius for o in big] == [500]
    assert len(doc.objects()) == 3


def test_modify_move_delete_and_record(doc):
    c = doc.add.circle([0, 0], 100)
    c.modify(radius=250, attrs={"line_weight": 1.0})
    assert c.radius == 250 and c.attrs["line_weight"] == 1.0
    c.move(10, 20)
    c.refresh()
    assert c["center"] == [10.0, 20.0]
    c.set_record(Note="hello")
    assert c.refresh().record["Note"] == "hello"
    c.modify(layer=doc.layers[0], class_=doc.classes[0])
    c.delete()
    assert doc.summary()["object_count"] == 0
    assert doc.undo() and doc.redo()
    assert doc.summary()["object_count"] == 0


def test_undo_group_is_one_step(doc):
    with doc.undo_group("two circles"):
        a = doc.add.circle([0, 0], 10)
        b = doc.add.circle([100, 0], 10)
        with doc.undo_group():  # nested groups join the outer one
            c = doc.add.line([0, 0], [1, 1])
        with pytest.raises(kadakas.KadakasError):
            a.id  # noqa: B018 - not known yet
        assert doc.in_undo_group
    assert doc.summary()["object_count"] == 3
    assert (a.id, b.id, c.id) == tuple(o.id for o in doc.objects())
    assert a.object().kind == "circle" and "Pending(" in repr(a)
    assert doc.undo()
    assert doc.summary()["object_count"] == 0
    assert not doc.undo()  # one undo step took back all of it
    assert doc.redo()
    assert doc.summary()["object_count"] == 3


def test_undo_group_on_error_sends_nothing(doc):
    with pytest.raises(RuntimeError):
        with doc.undo_group():
            doc.add.circle([0, 0], 10)
            raise RuntimeError("stop")
    assert doc.summary()["object_count"] == 0
    assert not doc.in_undo_group
    with doc.undo_group():
        pass  # an empty group sends nothing
    with pytest.raises(KadakasError):
        with doc.undo_group():
            doc.plants.place(at=[0, 0])
    with pytest.raises(KadakasError):
        with doc.undo_group():
            doc.add_layer("x")
    with pytest.raises(KadakasError):
        with doc.undo_group():
            doc.add_class("x")


def test_errors_carry_code_and_method(doc):
    missing = "00000000-0000-0000-0000-0000000003e7"
    with pytest.raises(KadakasError) as e:
        doc.get(missing)
    assert e.value.code == 2 and missing in str(e.value) and e.value.method == "objects.get"
    assert not e.value.is_forbidden
    with pytest.raises(KadakasError) as e:
        doc.call("nothing.here")
    assert e.value.code == -32601


def test_selection_and_groups(doc):
    a, b = doc.add.circle([0, 0], 1), doc.add.circle([5, 0], 1)
    assert len(doc.selection) == 0
    doc.selection.set([a, b.id])
    assert doc.selection.ids() == [a.id, b.id]
    assert [o.id for o in doc.selection] == [a.id, b.id]
    gid = doc.group([a, b], name="pair")
    assert doc.get(gid).kind == "group"
    doc.selection.clear()
    assert doc.selection.ids() == []


def test_plants_and_reports(doc):
    oak = doc.plants.create_style("Oak", spread=4000, height=8000)["id"]
    one = doc.plants.place(style=oak, at=[0, 0])
    assert len(one) == 1
    many = doc.plants.place("grid", style="Oak", min=[0, 0], max=[4000, 4000], spacing=2000)
    assert len(many) > 1
    assert doc.report.plants()["rows"]
    assert doc.plants.schedule()["total_count"] == 1 + len(many)
    assert isinstance(doc.report.plant_schedule(csv=True)["csv"], str)
    assert doc.plants.styles()
    doc.add.rect([0, 0], [1000, 1000])
    assert "rows" in doc.report.areas() and doc.report.hardscape() is not None


def test_worksheets(doc):
    sheet = doc.worksheets.create("Maths", cells={"A1": 2, "A2": 3, "A3": "=A1*A2"})
    assert doc.worksheets.evaluate(sheet)["cells"]["A3"] == 6
    doc.worksheets.set_cell(sheet, "A1", 5)
    assert doc.worksheets.evaluate("Maths")["cells"]["A3"] == 15
    assert "15" in doc.worksheets.csv(sheet)
    assert any(w for w in doc.worksheets.list())
    table = doc.worksheets.place(sheet, [0, 0])
    assert doc.get(table).kind


def test_import_export_and_files(doc, tmp_path: Path):
    geo = tmp_path / "in.geojson"
    geo.write_text(json.dumps({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString", "coordinates": [[0, 0], [10, 10]]}}]}))
    res = doc.import_file(geo)
    assert res["imported"] >= 1
    out = tmp_path / "out.dxf"
    doc.export_file(out)
    assert out.stat().st_size > 0
    saved = tmp_path / "saved.json"
    doc.save(saved)
    count = doc.summary()["object_count"]
    doc.add.circle([0, 0], 1)
    doc.open(saved)
    assert doc.summary()["object_count"] == count
    assert doc.to_json()["objects"]


def test_render(doc, tmp_path: Path):
    doc.add.rect([0, 0], [1000, 1000])
    doc.add.circle([500, 500], 200)
    png = doc.render_png(tmp_path / "view.png", width=200, height=150)
    assert png[:8] == b"\x89PNG\r\n\x1a\n" and (tmp_path / "view.png").read_bytes() == png
    svg = doc.render_svg(tmp_path / "plan.svg", width=300)
    assert "<svg" in svg
    assert (tmp_path / "plan.svg").read_text(encoding="utf-8") == svg


def test_open_a_document_file_and_describe(tmp_path: Path):
    saved = tmp_path / "start.json"
    with kadakas.open(allow_dir=[tmp_path]) as d:
        d.add.circle([0, 0], 7)
        d.save(saved)
    with kadakas.open(saved, allow_dir=[tmp_path]) as d:
        assert d.objects()[0].radius == 7
        assert "methods" in d.describe() and "millimeters" in d.describe()["units"]
        assert d.call("document.summary")["object_count"] == 1


def test_a_missing_program_is_reported():
    with pytest.raises(kadakas.ConnectionFailed):
        kadakas.open(binary="/no/such/kadakas")
