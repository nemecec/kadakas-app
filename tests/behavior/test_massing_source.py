"""Scenario: building outlines are imported from a survey, and massing models are made from them
with `delete_source: false` (the outlines stay in the drawing). A later revision of the survey
moves a corner of one outline. The outline must stay the object of the survey, and the model must
not look like an edit of it."""

from pathlib import Path

FIXTURES = Path(__file__).parent / "fixtures"
REV1 = FIXTURES / "survey_rev1.dxf"
REV2 = FIXTURES / "survey_rev2.dxf"
DRAWING = "guid:{6D6B37CD-4A03-47AD-B02E-199CB5C271A1}"


def building(cad):
    cad.result("import.file", {"path": str(REV1), "options": {"layer_mode": "classes"}})
    return cad.result("objects.query", {"criteria": 'Class = "BUILDING"'})["ids"][0]


def test_the_source_stays_the_survey_object_and_the_model_has_no_copy_of_its_tag(cad):
    src = building(cad)
    before = cad.result("objects.get", {"id": src})
    r = cad.result("massing.from_shapes", {"ids": [src], "default_height": 4500, "delete_source": False})
    made = r["created"][0]
    assert made["follows_source"] is False and "massing.follow_sources" in r["note"]
    # The source is unchanged, with its tag.
    assert cad.result("objects.get", {"id": src}) == before
    model = cad.result("objects.get", {"id": made["id"]})
    assert "import" not in model["data"], "a model made from a shape is new work"
    assert model["data"]["massing_source"] == src
    # With delete_source (the default) there is no link.
    other = cad.result("massing.from_shapes", {"ids": [src]})
    assert other["created"] and "replaced" in other["note"]


def test_a_new_revision_moves_the_corner_without_a_conflict_and_the_model_follows_on_request(cad):
    src = building(cad)
    model = cad.result("massing.from_shapes", {"ids": [src], "default_height": 4500, "delete_source": False})["created"][0]["id"]
    footprint = cad.result("objects.get", {"id": model})["shape"]["footprint"]
    up = cad.result("import.update", {"path": str(REV2), "drawing": DRAWING})
    assert up["counts"]["conflicts"] == 0, [c for c in up["changes"] if c["conflict"]]
    change = next(c for c in up["changes"] if c["layer"] == "BUILDING")
    assert change["objects"] == 1 and change["object_ids"] == [src]
    cad.result("import.apply", {"update_id": up["update_id"], "accept": "all"})
    moved = cad.result("objects.get", {"id": src})["shape"]["points"]
    assert moved != [v["p"] if isinstance(v, dict) else v for v in footprint], "the corner of the outline moved"
    # The model stays as it was until it is asked to follow.
    assert cad.result("objects.get", {"id": model})["shape"]["footprint"] == footprint
    r = cad.result("massing.follow_sources", {})
    assert r["changed"] == [model] and r["unchanged"] == [] and r["source_missing"] == []
    after = cad.result("objects.get", {"id": model})["shape"]["footprint"]
    assert [v["p"] if isinstance(v, dict) else v for v in after] == moved
    assert cad.result("massing.follow_sources", {})["unchanged"] == [model]
    # One undo step; and a model whose shape is gone says so.
    assert cad.result("undo")["done"]
    assert cad.result("objects.get", {"id": model})["shape"]["footprint"] == footprint
    cad.result("execute", {"command": {"op": "delete_objects", "ids": [src]}})
    assert cad.result("massing.follow_sources", {"ids": [model]})["source_missing"] == [model]
