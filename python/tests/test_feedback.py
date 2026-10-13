"""``doc.feedback``: bug reports and feature requests (API-47). Nothing is sent: sending is off."""

from __future__ import annotations

import uuid

import pytest

from kadakas.errors import KadakasError


def test_feature_request_and_bug_go_to_the_outbox(doc):
    title = f"Offset a polyline to both sides {uuid.uuid4()}"
    r = doc.feedback.feature_request(title, "one call for both sides", wanted="offset both", attach=False)
    assert r["status"] == "pending" and r["merged"] is False
    again = doc.feedback.feature_request(title, attach=False)
    assert again["merged"] is True and again["count"] == 2 and again["id"] == r["id"]
    bug = doc.feedback.bug(f"Fillet fails {uuid.uuid4()}", method="edit.fillet", severity="low", attach=False)
    listing = doc.feedback.list()
    ids = [x["id"] for x in listing["reports"]]
    assert {r["id"], bug["id"]} <= set(ids)
    assert listing["sending"]["enabled"] is False
    got = doc.feedback.get(r["id"])
    assert got["kind"] == "feature_request" and got["wanted"] == "offset both"
    assert doc.feedback.withdraw(bug["id"])["status"] == "withdrawn"


def test_a_report_with_attachments_waits_for_a_human(doc):
    doc.execute({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1000, 500]}})
    r = doc.feedback.bug(f"Odd result {uuid.uuid4()}")
    assert r["status"] == "needs_review"
    assert {"excerpt", "render", "calls"} <= set(r["attachments"])
    assert doc.feedback.get(r["id"])["status"] == "needs_review"
    assert not hasattr(doc.feedback, "approve") and not hasattr(doc.feedback, "send")


def test_errors_are_raised(doc):
    with pytest.raises(KadakasError):
        doc.feedback.submit("idea", "x")
    with pytest.raises(KadakasError):
        doc.feedback.get("fb-nope")
