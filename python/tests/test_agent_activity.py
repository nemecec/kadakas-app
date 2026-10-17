"""The activity log of the agent (AGR-05, AGR-06) through the Python package: every call or script
run that changes the document is one step, with a summary and the IDs of its objects."""

from __future__ import annotations

import pytest

from kadakas import KadakasError

ADD_CIRCLES = (
    "const ids = []; for (let i = 0; i < args.n; i++) ids.push(kadakas.call('execute', "
    "{command: {op: 'add_object', shape: {kind: 'circle', center: [i * 100, 0], radius: 10}}}).created[0]); "
)


def test_each_call_that_changes_the_document_is_a_step(doc):
    assert doc.agent_activity()["steps"] == []
    doc.summary()
    rect = doc.add.rect([0, 0], [10, 10])
    doc.add.rect([100, 0], [110, 10])
    log = doc.agent_activity()
    assert log["total"] == 2 and len(log["steps"]) == 2
    first = log["steps"][0]
    assert first["method"] == "execute" and first["state"] == "active" and first["revertible"] is True
    assert first["summary"] == "Added 1 object (1 rect)"
    assert first["counts"]["objects"] == {"added": 1, "changed": 0, "removed": 0}
    one = doc.agent_activity(first["id"])
    assert one["objects"]["added"] == [str(rect)]
    assert one["objects"]["changed"] == [] and one["objects"]["removed"] == []
    assert len(doc.agent_activity(since=first["id"])["steps"]) == 1
    assert len(doc.agent_activity(limit=1)["steps"]) == 1


def test_a_script_run_is_one_step(open_doc):
    open_doc.script_eval(ADD_CIRCLES + "return ids;", args={"n": 5})
    log = open_doc.agent_activity()
    assert len(log["steps"]) == 1
    step = log["steps"][0]
    assert step["method"] == "script.eval" and step["counts"]["objects"]["added"] == 5


def test_undo_by_the_agent_marks_the_step_undone_and_makes_no_step(doc):
    doc.add.rect([0, 0], [10, 10])
    doc.api.undo()
    log = doc.agent_activity()
    assert len(log["steps"]) == 1 and log["steps"][0]["state"] == "undone"
    assert log["steps"][0]["revertible"] is False


def test_a_version_is_saved_before_the_first_change_of_a_session(doc):
    assert doc.api.version_list()["count"] == 0
    doc.add.rect([0, 0], [10, 10])
    versions = doc.api.version_list()
    assert versions["count"] == 1 and versions["versions"][0]["name"] == "Before agent session"
    step = doc.agent_activity()["steps"][0]
    assert step["session_version"] == versions["versions"][0]["id"]
    doc.add.rect([50, 0], [60, 10])
    assert doc.api.version_list()["count"] == 1, "one version for each session"


def test_wrong_parameters_are_named(doc):
    with pytest.raises(KadakasError) as e:
        doc.api.agent_activity(stepp=1)
    assert "step" in str(e.value)
    with pytest.raises(KadakasError) as e:
        doc.agent_activity(99)
    assert "no step 99" in str(e.value)
