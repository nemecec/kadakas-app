"""Scenario: edge cases that used to corrupt documents or lose data, now refused or handled."""

import json
import math

from helpers import line_cmd, rect_cmd


def ids(cad):
    return [o["id"] for o in cad.result("objects.list")]


def test_moving_the_same_id_twice_moves_it_once(cad):
    (a,) = cad.execute(line_cmd(0, 0, 10, 0))
    cad.execute({"op": "move_objects", "ids": [a, a], "delta": [5, 0]})
    assert cad.result("objects.get", {"id": a})["shape"]["a"] == [5, 0]
    cad.result("undo")
    assert cad.result("objects.get", {"id": a})["shape"]["a"] == [0, 0]


def test_deleting_the_same_id_twice_is_fine_and_undo_restores_the_z_order(cad):
    made = [cad.execute(line_cmd(i, 0, i, 5))[0] for i in range(12)]
    cad.execute({"op": "delete_objects", "ids": [made[2], made[2], made[7], made[0], made[10], made[3], made[4],
                                                  made[5], made[6], made[8], made[9]]})
    assert ids(cad) == [made[1], made[11]]
    cad.result("undo")
    assert ids(cad) == made
    cad.result("redo")
    assert ids(cad) == [made[1], made[11]]


def test_overflow_to_infinity_is_refused_and_the_document_still_saves_and_opens(make_serve, tmp_path):
    cad = make_serve()
    (a,) = cad.execute(line_cmd(0, 0, 1e308, 0))
    err = cad.error("execute", {"command": {"op": "move_objects", "ids": [a], "delta": [1e308, 0]}})
    assert "finite" in err["message"]
    path = tmp_path / "big.json"
    assert cad.result("file.save", {"path": str(path)}) == {"ok": True}
    other = make_serve()
    assert other.result("file.open", {"path": str(path)}) == {"ok": True}
    assert other.result("objects.get", {"id": a})["shape"]["b"][0] == 1e308


def test_non_finite_numbers_in_a_request_are_a_parse_error(cad):
    # Python writes NaN, which is not JSON. The server must refuse the line, not store it.
    cad.send_raw('{"jsonrpc":"2.0","id":1,"method":"execute","params":{"command":{"op":"add_object",'
                 '"shape":{"kind":"circle","center":[NaN,0],"radius":1}}}}')
    resp = json.loads(cad.read_line())
    assert resp["error"]["code"] == -32700
    assert cad.result("document.summary")["object_count"] == 0


def test_arc_needs_a_non_zero_sweep_and_a_full_circle_is_allowed(cad):
    zero = {"op": "add_object", "shape": {"kind": "arc", "center": [0, 0], "radius": 5, "start": 1, "end": 1}}
    assert "sweep" in cad.error("execute", {"command": zero})["message"]
    full = {"op": "add_object", "shape": {"kind": "arc", "center": [0, 0], "radius": 5, "start": 0, "end": 2 * math.pi}}
    (a,) = cad.execute(full)
    # The drawing bounds use the true extent of the arc: here the whole circle.
    bounds = cad.result("document.summary")["bounds"]
    assert bounds["min"] == [-5, -5] and bounds["max"] == [5, 5]
    # A quarter arc in the upper right does not reach -X or -Y.
    cad.execute({"op": "delete_objects", "ids": [a]})
    quarter = {"op": "add_object", "shape": {"kind": "arc", "center": [0, 0], "radius": 100, "start": 0, "end": math.pi / 2}}
    cad.execute(quarter)
    bounds = cad.result("document.summary")["bounds"]
    assert bounds["min"][0] > -1 and bounds["min"][1] > -1


def test_set_data_merges_removes_and_undoes(cad):
    (a,) = cad.execute(rect_cmd(0, 0, 10, 10))
    cad.execute({"op": "set_data", "id": a, "set": {"Pset_Wall": {"FireRating": "REI60"}, "tag": "W1"}})
    cad.execute({"op": "set_data", "id": a, "set": {"tag": "W2"}, "remove": ["Pset_Wall"]})
    assert cad.result("objects.get", {"id": a})["data"] == {"tag": "W2"}
    cad.result("undo")
    assert cad.result("objects.get", {"id": a})["data"]["tag"] == "W1"
    cad.result("undo")
    assert "data" not in cad.result("objects.get", {"id": a})


def test_old_files_without_next_id_load_and_never_reuse_ids(make_serve, tmp_path):
    doc = make_serve().doc()
    del doc["next_id"]
    doc["objects"] = [{"id": 2, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]
    old = tmp_path / "old.json"
    old.write_text(json.dumps(doc))
    cad = make_serve(old)
    (new,) = cad.execute(rect_cmd(0, 0, 1, 1))
    assert new != 2


def test_saving_twice_keeps_the_previous_file_as_bak(make_serve, tmp_path):
    path = tmp_path / "d.json"
    cad = make_serve()
    cad.result("file.save", {"path": str(path)})
    cad.execute(rect_cmd(0, 0, 1, 1))
    cad.result("file.save", {"path": str(path)})
    assert len(json.loads(path.read_text())["objects"]) == 1
    assert json.loads((tmp_path / "d.json.bak").read_text())["objects"] == []
    assert sorted(p.name for p in tmp_path.iterdir()) == ["d.json", "d.json.bak"]


def test_a_new_layer_can_be_a_sheet(cad):
    (lid,) = cad.execute({"op": "add_layer", "name": "A1 sheet", "kind": "sheet"})
    kinds = {l["id"]: l["kind"] for l in cad.doc()["layers"]}
    assert kinds[lid] == "sheet" and kinds[1] == "design"


def test_ids_beyond_32_bits_survive_a_round_trip(make_serve, tmp_path):
    doc = make_serve().doc()
    big = 2**32 + 5
    doc["objects"] = [{"id": big, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]
    p = tmp_path / "big-id.json"
    p.write_text(json.dumps(doc))
    cad = make_serve(p)
    (lid,) = cad.execute({"op": "add_layer", "name": "L"})
    assert lid > big
    assert cad.result("objects.get", {"id": big})["id"] == big
