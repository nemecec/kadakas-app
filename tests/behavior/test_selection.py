"""Scenario: a designer selects objects, then edits the drawing. The selection must follow the objects that still exist."""

from helpers import rect_cmd


def test_selection_starts_empty_and_round_trips(cad):
    assert cad.result("selection.get") == {"ids": []}
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    b = cad.execute(rect_cmd(2, 2, 3, 3))[0]
    assert cad.result("selection.set", {"ids": [a, b]}) == {"ids": [a, b]}
    assert cad.result("selection.get") == {"ids": [a, b]}


def test_selection_set_with_unknown_id_fails_and_keeps_old_selection(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    cad.result("selection.set", {"ids": [a]})
    error = cad.error("selection.set", {"ids": [a, 999]})
    assert error["code"] == 2
    assert cad.result("selection.get") == {"ids": [a]}


def test_selection_is_pruned_when_a_selected_object_is_deleted(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    b = cad.execute(rect_cmd(2, 2, 3, 3))[0]
    cad.result("selection.set", {"ids": [a, b]})
    cad.execute({"op": "delete_objects", "ids": [a]})
    assert cad.result("selection.get") == {"ids": [b]}


def test_selection_is_pruned_when_an_add_is_undone(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    cad.result("selection.set", {"ids": [a]})
    cad.result("undo")
    assert cad.result("selection.get") == {"ids": []}


def test_selection_survives_edits_to_the_selected_object(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    cad.result("selection.set", {"ids": [a]})
    cad.execute({"op": "move_objects", "ids": [a], "delta": [5, 5]})
    assert cad.result("selection.get") == {"ids": [a]}


def test_undo_of_a_delete_does_not_restore_the_selection(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    cad.result("selection.set", {"ids": [a]})
    cad.execute({"op": "delete_objects", "ids": [a]})
    cad.result("undo")
    assert cad.result("selection.get") == {"ids": []}


def test_opening_a_new_document_clears_the_selection(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    cad.result("selection.set", {"ids": [a]})
    doc = cad.doc()
    cad.result("document.load", {"document": doc})
    assert cad.result("selection.get") == {"ids": []}
    assert cad.result("document.summary")["can_undo"] is False
