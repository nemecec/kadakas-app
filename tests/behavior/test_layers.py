"""Scenario: a designer locks, hides and removes layers and classes. Objects are kept, only their behavior changes."""

import pytest

from helpers import plant_cmd, rect_cmd, svg_object_ids, uid


@pytest.fixture
def locked_rect(cad):
    """A rectangle on layer 1, and layer 1 locked."""
    oid = cad.execute(rect_cmd(0, 0, 1000, 1000))[0]
    cad.execute({"op": "modify_layer", "id": uid(1), "locked": True})
    return oid


@pytest.mark.parametrize("make_command", [
    lambda oid: {"op": "move_objects", "ids": [oid], "delta": [10, 0]},
    lambda oid: {"op": "delete_objects", "ids": [oid]},
    lambda oid: {"op": "modify_object", "id": oid, "attrs": {"line_weight": 1.0}},
    lambda oid: {"op": "modify_object", "id": oid, "shape": {"kind": "circle", "center": [0, 0], "radius": 5}},
    lambda oid: rect_cmd(5, 5, 6, 6, layer=uid(1)),
])
def test_locked_layer_blocks_edits_and_leaves_document_unchanged(cad, locked_rect, make_command):
    before = cad.doc()
    error = cad.error("execute", {"command": make_command(locked_rect)})
    assert error["code"] == 5
    assert "locked" in error["message"]
    assert cad.doc() == before


def test_unlocking_a_layer_allows_edits_again(cad, locked_rect):
    cad.execute({"op": "modify_layer", "id": uid(1), "locked": False})
    cad.execute({"op": "move_objects", "ids": [locked_rect], "delta": [10, 0]})
    (obj,) = cad.result("objects.list")
    assert obj["shape"]["min"] == [10.0, 0.0]


def test_locked_objects_can_still_be_selected(cad, locked_rect):
    cad.result("selection.set", {"ids": [locked_rect]})
    assert cad.result("selection.get") == {"ids": [locked_rect]}


def test_hidden_class_removes_its_objects_from_render_but_keeps_them(cad):
    notes = cad.execute({"op": "add_class", "name": "Notes"})[0]
    visible_id = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    hidden_id = cad.execute(rect_cmd(2, 2, 3, 3, **{"class": notes}))[0]
    before = set(svg_object_ids(cad.result("render.svg", {"width": 400})["svg"]))
    assert before == {str(visible_id), str(hidden_id)}

    cad.execute({"op": "modify_class", "id": notes, "visible": False})
    svg = cad.result("render.svg", {"width": 400})["svg"]
    assert svg_object_ids(svg) == [str(visible_id)]
    assert len(cad.result("objects.list")) == 2, "hiding a class does not delete its objects"


def test_hidden_layer_removes_its_objects_from_render(cad):
    site = cad.execute({"op": "add_layer", "name": "Site"})[0]
    base = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    cad.execute({"op": "set_active", "layer": site})
    site_obj = cad.execute(plant_cmd(10, 10))[0]
    cad.execute({"op": "modify_layer", "id": site, "visible": False})
    assert svg_object_ids(cad.result("render.svg")["svg"]) == [str(base)]
    assert [o["id"] for o in cad.result("objects.list", {"layer": site})] == [site_obj]


def test_cannot_delete_a_layer_that_still_has_objects(cad):
    site = cad.execute({"op": "add_layer", "name": "Site"})[0]
    cad.execute({"op": "set_active", "layer": site})
    cad.execute(rect_cmd(0, 0, 1, 1))
    error = cad.error("execute", {"command": {"op": "delete_layer", "id": site}})
    assert error["code"] == 5
    assert "not empty" in error["message"]


def test_deleting_the_active_empty_layer_moves_active_to_first_layer(cad):
    site = cad.execute({"op": "add_layer", "name": "Site"})[0]
    cad.execute({"op": "set_active", "layer": site})
    cad.execute({"op": "delete_layer", "id": site})
    summary = cad.result("document.summary")
    assert [layer["id"] for layer in summary["layers"]] == [uid(1)]
    assert summary["active_layer"] == uid(1)


def test_the_last_layer_cannot_be_deleted(cad):
    error = cad.error("execute", {"command": {"op": "delete_layer", "id": uid(1)}})
    assert error["code"] == 3
    assert "at least one layer" in error["message"]
