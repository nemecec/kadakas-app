"""Scenario: a designer makes changes, steps back and forth through history, and makes new changes."""

from helpers import line_cmd, rect_cmd, strip_next_id, uid


def test_undo_then_redo_walks_history_exactly(cad):
    snapshots = [strip_next_id(cad.doc())]
    first = cad.execute(rect_cmd(0, 0, 1000, 1000))[0]
    snapshots.append(strip_next_id(cad.doc()))
    second = cad.execute(rect_cmd(2000, 0, 3000, 1000))[0]
    snapshots.append(strip_next_id(cad.doc()))
    cad.execute({"op": "move_objects", "ids": [first], "delta": [0, 500]})
    snapshots.append(strip_next_id(cad.doc()))
    cad.execute({"op": "delete_objects", "ids": [second]})
    snapshots.append(strip_next_id(cad.doc()))

    for expected in reversed(snapshots[:-1]):
        assert cad.result("undo") == {"done": True}
        assert strip_next_id(cad.doc()) == expected
    assert cad.result("undo") == {"done": False}

    for expected in snapshots[1:]:
        assert cad.result("redo") == {"done": True}
        assert strip_next_id(cad.doc()) == expected
    assert cad.result("redo") == {"done": False}


def test_undo_of_delete_brings_back_objects_in_the_same_z_order(cad):
    ids = [cad.execute(line_cmd(i, 0, i, 10))[0] for i in range(4)]
    before = [o["id"] for o in cad.result("objects.list")]
    cad.execute({"op": "delete_objects", "ids": [ids[1], ids[2]]})
    cad.result("undo")
    assert [o["id"] for o in cad.result("objects.list")] == before


def test_new_command_after_undo_clears_redo(cad):
    cad.execute(rect_cmd(0, 0, 1, 1))
    cad.execute(rect_cmd(5, 5, 6, 6))
    cad.result("undo")
    assert cad.result("document.summary")["can_redo"] is True
    cad.execute(rect_cmd(9, 9, 10, 10))
    assert cad.result("document.summary")["can_redo"] is False
    assert cad.result("redo") == {"done": False}


def test_undo_and_redo_on_empty_history_report_done_false(cad):
    before = cad.doc()
    assert cad.result("undo") == {"done": False}
    assert cad.result("redo") == {"done": False}
    assert cad.doc() == before


def test_each_command_is_one_undo_step_even_with_many_objects(cad):
    cad.execute({"op": "batch", "commands": [rect_cmd(0, 0, 1, 1), rect_cmd(2, 2, 3, 3), rect_cmd(4, 4, 5, 5)]})
    assert cad.result("undo") == {"done": True}
    assert cad.result("objects.list") == []
    assert cad.result("undo") == {"done": False}


def test_set_active_layer_is_undoable(cad):
    site = cad.execute({"op": "add_layer", "name": "Site"})[0]
    cad.execute({"op": "set_active", "layer": site})
    assert cad.result("document.summary")["active_layer"] == site
    cad.result("undo")
    assert cad.result("document.summary")["active_layer"] == uid(1)


def test_failed_command_leaves_document_and_undo_stack_unchanged(cad):
    cad.execute(rect_cmd(0, 0, 1, 1))
    before = cad.doc()
    error = cad.error("execute", {"command": {"op": "delete_objects", "ids": [uid(999)]}})
    assert error["code"] == 2
    assert cad.doc() == before
    assert cad.result("undo") == {"done": True}
    assert cad.result("undo") == {"done": False}


def test_noop_command_creates_no_undo_step(cad):
    cad.execute(rect_cmd(0, 0, 1, 1))
    oid = cad.result("objects.list")[0]["id"]
    cad.execute({"op": "move_objects", "ids": [oid], "delta": [0, 0]})
    cad.result("undo")
    assert cad.result("undo") == {"done": False}
