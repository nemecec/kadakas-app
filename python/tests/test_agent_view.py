"""The view methods of the agent (AGR-02, AGR-03) through the Python package. The headless server
has no window, so each call raises the error that names the app; the wrappers still turn objects
into IDs and leave out the arguments that were not given."""

from __future__ import annotations

import pytest

from kadakas import KadakasError


def test_view_methods_name_the_app_on_a_server_without_a_window(doc):
    rect = doc.add.rect([0, 0], [10, 10])
    calls = [
        lambda: doc.view_get(),
        lambda: doc.view_zoom(ids=[rect]),
        lambda: doc.view_zoom(box=[0, 0, 100, 100], margin=0.2),
        lambda: doc.view_pan(by=[5, 5]),
        lambda: doc.view_camera([1000, 0, 500], [0, 0, 0], projection="perspective"),
        lambda: doc.view_highlight(ids=[rect], text="Not paved", color="red", until="next_change"),
        lambda: doc.view_highlight_clear(),
        lambda: doc.view_highlight_clear(3),
    ]
    for call in calls:
        with pytest.raises(KadakasError) as e:
            call()
        assert "live window of the app" in str(e.value)


def test_a_wrong_name_is_refused_before_the_app(doc):
    with pytest.raises(KadakasError) as e:
        doc.api.view_zoom(idz=[])
    assert "ids" in str(e.value)
