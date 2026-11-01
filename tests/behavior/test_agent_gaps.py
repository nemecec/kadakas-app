"""Scenario (AGR-09): an agent builds a furniture model through `mesh.*` and meets three gaps.

It cannot delete an object by a listed method, an unknown method name gives no help, and
`selection.get` does not say which faces and edges the person picked. It also needs the camera of
the person to make a picture of the same view. The camera of the app itself (`view.get`) is tested
in the app crate, because only the app has a window."""

import base64

from helpers import rect_cmd, uid


def box(cad):
    """A model with one box: its id and the ids of its faces and edges."""
    model = cad.result("mesh.create", {"name": "Desk"})["id"]
    corner = lambda i: [100.0 if i >> k & 1 else 0.0 for k in range(3)]
    edges = [[corner(i), corner(i | bit)] for i in range(8) for bit in (1, 2, 4) if not i & bit]
    cad.result("mesh.add_edges", {"id": model, "edges": edges})
    ids = lambda m: [i["id"] for i in cad.result(m, {"id": model, "limit": 1000})["items"]]
    return model, ids("mesh.faces"), ids("mesh.edges")


# ---------------------------------------------------------------- objects.delete

def test_objects_delete_is_described_with_a_schema(cad):
    methods = {m["name"]: m["doc"] for m in cad.result("describe")["methods"]}
    assert "delete_objects" in methods["objects.delete"]
    schema = cad.result("describe", {"method": "objects.delete"})["params"]
    assert schema["required"] == ["ids"]


def test_objects_delete_is_one_undo_step_and_the_same_as_the_command(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    b = cad.execute(rect_cmd(2, 2, 3, 3))[0]
    c = cad.execute(rect_cmd(4, 4, 5, 5))[0]
    cad.result("selection.set", {"ids": [a, b]})
    full = cad.doc()
    out = cad.result("objects.delete", {"ids": [a, b]})
    assert out == {"count": 2, "deleted": [a, b]}
    assert cad.result("document.summary")["object_count"] == 1
    assert cad.result("selection.get")["ids"] == []
    assert cad.result("undo") == {"done": True}
    assert cad.doc() == full
    # The command form gives the same document, and an undo again gives back the same one.
    cad.result("objects.delete", {"ids": [c]})
    after_method = cad.doc()
    cad.result("undo")
    cad.execute({"op": "delete_objects", "ids": [c]})
    assert cad.doc() == after_method


def test_objects_delete_refuses_unknown_ids_and_deletes_nothing(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    error = cad.error("objects.delete", {"ids": [a, uid(999)]})
    assert error["code"] == 2 and error["data"]["error_id"] == "no_such_object"
    assert uid(999) in error["message"]
    assert cad.result("document.summary")["object_count"] == 1
    assert cad.error("objects.delete", {"ids": []})["data"]["error_id"] == "ids_empty"
    assert cad.error("objects.delete", {})["code"] == -32602
    assert cad.error("objects.delete", {"ids": [a], "idz": 1})["code"] == -32602


def test_objects_delete_follows_the_rules_of_a_locked_layer(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    layer = cad.result("document.summary")["layers"][0]["id"]
    cad.execute({"op": "modify_layer", "id": layer, "locked": True})
    by_command = cad.error("execute", {"command": {"op": "delete_objects", "ids": [a]}})
    by_method = cad.error("objects.delete", {"ids": [a]})
    assert (by_method["code"], by_method["message"]) == (by_command["code"], by_command["message"])
    assert cad.result("document.summary")["object_count"] == 1


def test_objects_delete_works_in_a_script_and_not_in_read_mode(cad):
    a = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    code = "return kadakas.call('objects.delete', {ids: args.ids});"
    refused = cad.error("script.eval", {"code": code, "args": {"ids": [a]}, "mode": "read"})
    assert "read-only" in refused["message"]
    assert cad.result("document.summary")["object_count"] == 1
    out = cad.result("script.eval", {"code": code, "args": {"ids": [a]}})
    assert out["result"]["count"] == 1 and out["undo_steps"] == 1
    assert cad.result("document.summary")["object_count"] == 0


def test_objects_delete_is_in_the_destructive_tool_of_mcp(mcp):
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    assert "- objects.delete:" in tools["document"]["description"]
    assert tools["document"]["annotations"]["destructiveHint"] is True
    assert tools["document"]["annotations"]["readOnlyHint"] is False
    assert "- objects.delete:" not in tools["objects"]["description"]
    assert tools["objects"]["annotations"]["readOnlyHint"] is True
    created = mcp.method_json("document", "execute", {"command": rect_cmd(0, 0, 1, 1)})["created"]
    out = mcp.method_json("document", "objects.delete", {"ids": created})
    assert out["count"] == 1
    # The read-only tool does not take the method.
    assert mcp.method("objects", "objects.delete", {"ids": created})["isError"] is True


# ---------------------------------------------------------------- unknown methods

def test_an_unknown_method_names_the_closest_listed_methods(cad):
    listed = {m["name"] for m in cad.result("describe")["methods"]}
    for guess in ("object.delete", "objects.remove", "delete_objects", "objects.erase"):
        error = cad.error(guess, {"ids": []})
        assert error["code"] == -32601 and error["data"]["kind"] == "method_not_found"
        assert error["message"].startswith(f"unknown method: {guess}")
        assert "objects.delete {ids}" in error["message"] and '"delete_objects"' in error["message"]
        closest = error["data"]["closest"]
        assert 0 < len(closest) <= 3 and set(closest) <= listed, closest
    typo = cad.error("mesh.facez")
    assert "Closest listed methods: mesh.faces" in typo["message"]
    assert "describe lists the methods" in cad.error("qqqqqqqqqqqq")["message"]


# ---------------------------------------------------------------- selection.get with parts

def test_selection_get_has_empty_maps_when_nothing_is_selected(cad):
    assert cad.result("selection.get") == {"ids": [], "faces": {}, "edges": {}}


def test_selection_holds_faces_and_edges_by_the_ids_of_the_mesh_methods(cad):
    model, faces, edges = box(cad)
    want = {"ids": [], "faces": {model: sorted(faces[:2])}, "edges": {model: [edges[3]]}}
    assert cad.result("selection.set", {"faces": {model: faces[:2][::-1]}, "edges": {model: [edges[3]]}}) == want
    assert cad.result("selection.get") == want
    assert cad.result("mesh.faces", {"id": model, "ids": [faces[0]]})["items"][0]["id"] == faces[0]
    # Whole objects replace the parts.
    cad.result("selection.set", {"ids": [model]})
    assert cad.result("selection.get") == {"ids": [model], "faces": {}, "edges": {}}


def test_selection_set_checks_parts_and_keeps_the_old_selection_on_a_refusal(cad):
    model, faces, edges = box(cad)
    rect = cad.execute(rect_cmd(0, 0, 1, 1))[0]
    cad.result("selection.set", {"edges": {model: [edges[0]]}})
    assert cad.error("selection.set", {"faces": {model: [99999]}})["data"]["error_id"] == "no_such_face"
    assert cad.error("selection.set", {"edges": {model: [99999]}})["data"]["error_id"] == "no_such_edge"
    assert cad.error("selection.set", {"faces": {rect: [1]}})["data"]["error_id"] == "not_a_surface_model"
    assert cad.error("selection.set", {"ids": [rect], "faces": {model: [faces[0]]}})["code"] == -32602
    assert cad.error("selection.set", {})["code"] == -32602
    assert cad.result("selection.get")["edges"] == {model: [edges[0]]}


def test_selected_parts_follow_undo_and_delete(cad):
    model, faces, _ = box(cad)
    cad.result("selection.set", {"faces": {model: faces}})
    cad.result("undo")
    assert cad.result("selection.get")["faces"] == {}
    cad.result("redo")
    cad.result("selection.set", {"faces": {model: faces[:1]}})
    cad.result("objects.delete", {"ids": [model]})
    assert cad.result("selection.get") == {"ids": [], "faces": {}, "edges": {}}


# ---------------------------------------------------------------- the camera

VIEW_GET_CAMERA = {
    # What `view.get` gives in 3D: the keys of the camera of render.png, and the old read-only keys.
    "view": "right_isometric", "projection": "perspective", "render_mode": "shaded",
    "eye": [8000.0, -9000.0, 6000.0], "target": [2000.0, 1500.0, 0.0], "up": [0.0, 0.0, 1.0],
    "fov_deg": 45.0, "fov": 45.0, "distance": 13000.0, "yaw": -0.85, "pitch": 0.5, "aspect": 1.5,
    "visible_box": {"min": [-5000.0, -5000.0], "max": [9000.0, 8000.0]},
}


def png(answer):
    return base64.b64decode(answer["png_base64"])


def test_render_png_takes_the_camera_of_view_get_as_it_is(cad):
    model, _, _ = box(cad)
    size = {"width": 96, "height": 64}
    as_given = cad.result("render.png", {**size, "camera": VIEW_GET_CAMERA})
    keys = ("target", "yaw", "pitch", "distance", "fov", "projection")
    spelled = cad.result("render.png", {**size, "camera": {k: VIEW_GET_CAMERA[k] for k in keys}})
    assert png(as_given) == png(spelled) and as_given["camera"] == spelled["camera"]
    assert abs(as_given["camera"]["yaw"] - VIEW_GET_CAMERA["yaw"]) < 1e-6
    # A null visible_box is what view.get says when the camera looks past the horizon.
    cad.result("render.png", {**size, "camera": {**VIEW_GET_CAMERA, "visible_box": None}})
    # The camera of an answer round-trips as well, and mesh.render takes the same camera.
    again = cad.result("render.png", {**size, "camera": as_given["camera"]})
    assert png(again) == png(as_given)
    mesh = cad.result("mesh.render", {"id": model, **size, "camera": VIEW_GET_CAMERA})
    assert mesh["camera"]["target"] == VIEW_GET_CAMERA["target"]
    # A different camera makes a different picture.
    other = cad.result("render.png", {**size, "camera": {**VIEW_GET_CAMERA, "yaw": 1.0}})
    assert png(other) != png(as_given)
    # A typo is still an error that names the key.
    typo = cad.error("render.png", {**size, "camera": {**VIEW_GET_CAMERA, "yawn": 1.0}})
    assert typo["code"] == -32602 and "yaw" in typo["message"]


def test_view_camera_takes_the_camera_of_view_get_in_both_forms(cad):
    # The headless server has no window: it names the app, after it has read the parameters.
    for camera in (VIEW_GET_CAMERA, {k: VIEW_GET_CAMERA[k] for k in ("target", "yaw", "pitch", "distance")}):
        error = cad.error("view.camera", camera)
        assert error["code"] == 1 and "live window of the app" in error["message"], error
    assert cad.error("view.camera", {"target": [0, 0, 0], "yawn": 1})["code"] == -32602
