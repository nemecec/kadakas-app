"""``doc.collab``: the wrapper around ``collab.*``. A fake transport stands in for the program;
the real thing is tested with a server in the kadakas-server repository (tests/e2e)."""

from __future__ import annotations

import pytest

from kadakas.document import Document


class Fake:
    """Answers collab calls like a program whose client goes online after two syncs."""

    def __init__(self) -> None:
        self.calls: list[tuple[str, dict | None]] = []
        self.syncs = 0

    def request(self, method, params=None):
        self.calls.append((method, params))
        if method == "collab.connect":
            return {"state": "syncing"}
        if method == "collab.sync":
            self.syncs += 1
            state = "online" if self.syncs >= 2 else "syncing"
            return {"status": {"state": state, "pending": 0}}
        return {"state": "online"}

    def close(self) -> None:
        pass


def test_connect_passes_only_the_given_values_and_waits_until_online():
    fake = Fake()
    doc = Document(fake)
    status = doc.collab.connect(url="ws://127.0.0.1:8787", token="t", doc="d", name="Ann")
    assert status["state"] == "online"
    method, params = fake.calls[0]
    assert method == "collab.connect"
    assert params == {"url": "ws://127.0.0.1:8787", "token": "t", "doc": "d", "name": "Ann"}
    assert [m for m, _ in fake.calls].count("collab.sync") == 2


def test_connect_without_waiting_returns_the_first_status():
    fake = Fake()
    status = Document(fake).collab.connect(wait=False)
    assert status == {"state": "syncing"}
    assert fake.calls == [("collab.connect", None)]


def test_wait_online_times_out():
    class Never(Fake):
        def request(self, method, params=None):
            if method == "collab.sync":
                return {"status": {"state": "offline", "last_error": "refused"}}
            return super().request(method, params)

    with pytest.raises(TimeoutError, match="refused"):
        Document(Never()).collab.wait_online(timeout=0.05)


def test_open_gives_the_token_in_the_environment_and_not_in_the_arguments(monkeypatch):
    seen = {}

    class Spy:
        def __init__(self, args, binary=None, env=None):
            seen.update(args=args, env=env)

        def request(self, *a, **k):
            return {}

        def close(self):
            pass

    import kadakas.document as d

    monkeypatch.setattr(d, "StdioTransport", Spy)
    d.open(allow_url=["maps.example.org"], allow_collab_url=["c.example.org"], collab_url="ws://127.0.0.1:8787", collab_token="secret")
    assert seen["args"] == ["--allow-url", "maps.example.org", "--allow-collab-url", "c.example.org"]
    assert seen["env"] == {"KADAKAS_COLLAB_URL": "ws://127.0.0.1:8787", "KADAKAS_COLLAB_TOKEN": "secret"}
    assert "secret" not in " ".join(seen["args"])
