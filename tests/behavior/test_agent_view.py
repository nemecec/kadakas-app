"""Scenario: an agent drives the view of the person and points (AGR-02, AGR-03).

Only the app has a window, a camera and a person. The headless server names the app. The app side
(frames, animation, the person who wins, Back, the pointers) is tested in the app crate through the
handler that the agent server calls. Here: what a script and an MCP client see of a server without
a window, and the parts that work everywhere (saved views in `view.set` and `view.list`)."""

import pytest

APP_METHODS = ["view.get", "view.zoom", "view.pan", "view.camera", "view.highlight", "view.highlight_clear"]


def test_the_methods_are_described(cad):
    desc = cad.result("describe")
    docs = {m["name"]: m["doc"] for m in desc["methods"]}
    for name in APP_METHODS:
        assert name in docs, name
        assert "app" in docs[name]
    assert "Back" in desc["review"]["agent_view"]["person_in_control"]
    assert desc["review"]["agent_view"]["bounds"]["highlights"] == 32
    assert "next_change" in docs["view.highlight"] and "person_navigating" in docs["view.zoom"]


@pytest.mark.parametrize("method,params", [
    ("view.get", {}),
    ("view.zoom", {"at": [0, 0]}),
    ("view.pan", {"by": [1, 1]}),
    ("view.camera", {"eye": [1000, 0, 0], "target": [0, 0, 0]}),
    ("view.highlight", {"at": [0, 0], "text": "Here"}),
    ("view.highlight_clear", {}),
])
def test_the_headless_server_names_the_app(cad, method, params):
    err = cad.error(method, params)
    assert err["code"] == 1
    assert "live window of the app" in err["message"] and "--agent-port" in err["message"]
    assert cad.result("document.summary")["object_count"] == 0


def test_an_unknown_parameter_is_named(cad):
    err = cad.error("view.zoom", {"idz": []})
    assert err["code"] == -32602 and "ids" in err["message"]


def test_saved_views_are_listed_and_view_set_takes_their_names(cad):
    assert cad.result("view.list")["saved_views"] == []
    cad.result("saved_view.save", {"name": "Overview", "zoom": 0.1, "pan": [5, 6]})
    assert cad.result("view.list")["saved_views"] == [{"name": "Overview"}]
    before = cad.result("document.summary")
    out = cad.result("view.set", {"view": "overview"})
    assert out["saved_view"] == "Overview" and out["layers_not_applied"] is False
    # Setting a view is never an edit.
    assert cad.result("document.summary") == before
    err = cad.error("view.set", {"view": "nowhere"})
    assert err["code"] == -32602


def test_the_view_tools_are_in_the_review_tool_of_mcp(mcp):
    tools = {t["name"]: t for t in mcp.request("tools/list")["result"]["tools"]}
    text = tools["review"]["description"]
    for name in APP_METHODS:
        assert f"- {name}:" in text, name
    assert "- view.screenshot:" in tools["render_view"]["description"]
    assert "- view.get:" not in tools["render_view"]["description"]


def test_view_get_describes_the_pointer_and_where_it_is_delivered(cad):
    """AGR-10: the pointer of the person is a field of view.get, and the text says who delivers it."""
    desc = cad.result("describe")
    docs = {m["name"]: m["doc"] for m in desc["methods"]}
    text = docs["view.get"]
    for word in ["pointer", "over_canvas", "canvas", "age_ms", "navigating", "ray", "hit", "ids", "mesh.pick", "null"]:
        assert word in text, word
    assert "modeler" in text and "kadakas serve" in text
    note = desc["review"]["agent_view"]["pointer"]
    assert "Privacy" in note and "not saved" in note and "collaborators" in note
    assert "Only the app delivers" in note
    recipes = " ".join(desc["review"]["agent_view"]["recipes"])
    assert "pointer.hit" in recipes and "pointer.ray" in recipes


def test_the_ray_of_the_pointer_goes_into_mesh_pick_as_it_is(cad):
    """The ray has the shape of the pointer: lists origin and direction. mesh.pick takes it as it is."""
    model = cad.result("mesh.create", {"name": "Box"})["id"]
    corner = lambda i: [100 if i & 1 else 0, 100 if i & 2 else 0, 100 if i & 4 else 0]
    edges = [[corner(i), corner(i | b)] for i in range(8) for b in (1, 2, 4) if not i & b]
    cad.result("mesh.add_edges", {"id": model, "edges": edges})
    ray = {"origin": [50.0, 50.0, 500.0], "direction": [0.0, 0.0, -1.0]}
    hit = cad.result("mesh.pick", ray)["hit"]
    assert hit["model"] == model and abs(hit["distance"] - 400.0) < 1e-6
    # The keys of the hit that the pointer reports are the keys of this answer.
    for key in ("model", "face", "point", "distance"):
        assert key in hit
    # A ray past the model hits nothing, which is a null hit.
    miss = cad.result("mesh.pick", {"origin": [900.0, 900.0, 500.0], "direction": [0.0, 0.0, -1.0]})
    assert miss["hit"] is None
