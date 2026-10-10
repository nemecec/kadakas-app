"""Scenario: a designer applies a multi-step change as one batch. If any step fails, nothing is kept."""

from helpers import rect_cmd, strip_next_id


def test_batch_failure_rolls_back_every_step(cad):
    keep = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    before = strip_next_id(cad.doc())
    error = cad.error("execute", {"command": {"op": "batch", "commands": [
        {"op": "add_layer", "name": "Site"},
        {"op": "set_active", "layer": 1},
        rect_cmd(5, 5, 6, 6),
        {"op": "move_objects", "ids": [keep], "delta": [100, 0]},
        {"op": "delete_objects", "ids": [999]},
    ]}})
    assert error["code"] == 2
    assert "999" in error["message"]
    assert strip_next_id(cad.doc()) == before


def test_failed_batch_is_not_an_undo_step(cad):
    cad.execute(rect_cmd(0, 0, 1, 1))
    cad.error("execute", {"command": {"op": "batch", "commands": [
        rect_cmd(2, 2, 3, 3), {"op": "delete_objects", "ids": [999]},
    ]}})
    assert cad.result("undo") == {"done": True}  # undoes the first rectangle only
    assert cad.result("undo") == {"done": False}


def test_failed_batch_keeps_the_redo_stack(cad):
    cad.execute(rect_cmd(0, 0, 1, 1))
    cad.result("undo")
    assert cad.result("document.summary")["can_redo"] is True
    cad.error("execute", {"command": {"op": "batch", "commands": [{"op": "delete_objects", "ids": [999]}]}})
    assert cad.result("document.summary")["can_redo"] is True
    assert cad.result("redo") == {"done": True}


def test_successful_batch_reports_every_created_id_in_order(cad):
    layer = cad.execute({"op": "add_layer", "name": "Planting"})[0]
    created = cad.execute({"op": "batch", "commands": [
        rect_cmd(0, 0, 1, 1, layer=layer),
        rect_cmd(2, 2, 3, 3, layer=layer),
    ]})
    assert len(created) == 2
    assert created == sorted(created)
    assert [o["id"] for o in cad.result("objects.list", {"layer": layer})] == created


def test_successful_batch_undoes_in_one_step(cad):
    cad.execute({"op": "batch", "commands": [
        {"op": "add_layer", "name": "Site"},
        {"op": "set_active", "layer": 2},
        rect_cmd(0, 0, 1, 1),
    ]})
    assert cad.result("undo") == {"done": True}
    summary = cad.result("document.summary")
    assert summary["object_count"] == 0
    assert [layer["name"] for layer in summary["layers"]] == ["Layer 1"]
    assert summary["active_layer"] == 1
