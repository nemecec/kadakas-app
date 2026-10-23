"""The modeler through the generated Python client (MOD-11): typed `mesh_*` methods for the
surface model, one undo step for each edit, an atomic batch, refusals with stable ids, and a
render that the agent can look at."""

from __future__ import annotations

import base64

import pytest

from kadakas import KadakasError


def rect(x0, y0, x1, y1):
    return [
        [[x0, y0, 0], [x1, y0, 0]],
        [[x1, y0, 0], [x1, y1, 0]],
        [[x1, y1, 0], [x0, y1, 0]],
        [[x0, y1, 0], [x0, y0, 0]],
    ]


def geometry(doc, model):
    api = doc.api
    return [
        api.mesh_vertices(id=model, limit=1000)["items"],
        api.mesh_edges(id=model, limit=1000)["items"],
        api.mesh_faces(id=model, limit=1000)["items"],
    ]


def make_box(doc):
    model = doc.api.mesh_create(name="Shed")["id"]
    drawn = doc.api.mesh_add_edges(id=model, edges=rect(0, 0, 4000, 3000))
    face = drawn["created"]["faces"][0]
    doc.api.mesh_push_pull(id=model, face=face, distance=2500)
    return model, face


def test_the_client_has_a_typed_method_for_every_mesh_call(doc):
    names = [n for n in dir(doc.api) if n.startswith("mesh_")]
    assert len(names) == 29
    assert "mesh_soften_edges" in names and "mesh_batch" in names and "mesh_render" in names
    assert "Soften every edge under an angle" in doc.api.mesh_soften_edges.__doc__


def test_draw_pull_and_check_a_solid(doc):
    model, _ = make_box(doc)
    summary = doc.api.mesh_summary(id=model)["models"][0]
    assert (summary["vertices"], summary["edges"], summary["faces"]) == (8, 12, 6)
    report = doc.api.mesh_solid_report(id=model)
    assert report["closed"] is True
    assert report["shells"][0]["state"] == "solid"
    bounds = doc.api.mesh_bounds(id=model)
    assert bounds["world"]["size"] == [4000.0, 3000.0, 2500.0]
    hit = doc.api.mesh_pick(origin=[2000, 1500, 9000], direction=[0, 0, -1])["hit"]
    assert hit["side"] == "front" and hit["point"] == [2000.0, 1500.0, 2500.0]
    near = doc.api.mesh_near(point=[2000, 1500, 2600], radius=200)
    assert near["faces"][0]["id"] == hit["face"]


def test_every_edit_is_one_undo_step(doc):
    model, face = make_box(doc)
    before = geometry(doc, model)
    for call in (
        lambda: doc.api.mesh_push_pull(id=model, face=face, distance=100),
        lambda: doc.api.mesh_transform(id=model, faces=[face], translate=[0, 0, 100]),
        lambda: doc.api.mesh_soften_edges(id=model, below_angle=100),
        lambda: doc.api.mesh_hide(id=model, faces=[face]),
        lambda: doc.api.mesh_copy(id=model, faces=[face], translate=[5000, 0, 0]),
    ):
        depth = doc.api.mesh_summary(id=model)["models"][0]["revision"]
        call()
        assert doc.api.mesh_summary(id=model)["models"][0]["revision"] == depth + 1
        doc.api.undo()
        assert geometry(doc, model) == before


def test_soften_edges_reports_how_many_edges_changed(doc):
    import math

    model = doc.api.mesh_create()["id"]
    points = [[1000 * math.cos(math.tau * i / 24), 1000 * math.sin(math.tau * i / 24), 0] for i in range(24)]
    doc.api.mesh_add_curve(id=model, points=points, closed=True, faceted=True)
    face = doc.api.mesh_faces(id=model)["items"][0]["id"]
    doc.api.mesh_push_pull(id=model, face=face, distance=2000)
    assert doc.api.mesh_soften_edges(id=model, below_angle=10)["changed"] == 0
    first = doc.api.mesh_soften_edges(id=model, below_angle=20)
    assert first["changed"] == 24 and first["softened"] == 24
    assert doc.api.mesh_soften_edges(id=model, below_angle=20)["changed"] == 0


def test_a_batch_is_atomic(doc):
    model = doc.api.mesh_create()["id"]
    before = geometry(doc, model)
    with pytest.raises(KadakasError) as e:
        doc.api.mesh_batch(
            id=model,
            edits=[
                {"method": "add_edges", "edges": rect(0, 0, 1000, 1000)},
                {"method": "push_pull", "face": 9999, "distance": 10},
            ],
        )
    assert e.value.data["error_id"] == "no_such_face" and e.value.data["step"] == 1
    assert geometry(doc, model) == before
    result = doc.api.mesh_batch(
        id=model,
        edits=[
            {"method": "add_edges", "edges": rect(0, 0, 1000, 1000)},
            {"method": "add_edges", "edges": rect(2000, 0, 3000, 1000)},
        ],
    )
    assert result["count"] == 2 and result["edits"][1]["counts"]["faces"] == 2
    doc.api.undo()
    assert geometry(doc, model) == before


def test_a_refusal_has_a_message_and_an_id(doc):
    model = doc.api.mesh_create()["id"]
    with pytest.raises(KadakasError) as e:
        doc.api.mesh_push_pull(id=model, face=42, distance=10)
    assert "face 42 does not exist" in str(e.value)
    assert e.value.code == 2 and e.value.kind == "not_found"
    assert e.value.data["error_id"] == "no_such_face" and e.value.data["face"] == 42


def test_edits_of_the_agent_show_in_the_activity_log(doc):
    model, _ = make_box(doc)
    methods = [s["method"] for s in doc.agent_activity()["steps"]]
    assert methods == ["mesh.create", "mesh.add_edges", "mesh.push_pull"]


def test_render_gives_a_png(doc):
    model, _ = make_box(doc)
    camera = {
        "target": [2000, 1500, 1000],
        "yaw": 0.8,
        "pitch": 0.5,
        "distance": 12000,
        "fov": 45,
        "projection": "perspective",
    }
    picture = doc.api.mesh_render(id=model, camera=camera, width=96, height=64)
    png = base64.b64decode(picture["png_base64"])
    assert png[1:4] == b"PNG"
    assert picture["camera"]["projection"] == "perspective"
