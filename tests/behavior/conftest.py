"""Fixtures: each test gets its own `kadakas` subprocess, so tests never share state."""

import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import pytest

from helpers import BIN, REPO, TEST_HOME, JsonRpc, McpClient


# Agents may only use some folders. The tests use pytest's temp folders and the files of the repository.
ALLOW = ["--allow-dir", str(Path(tempfile.gettempdir()).resolve()), "--allow-dir", str(REPO)]


@pytest.fixture(scope="session", autouse=True)
def built_cli():
    """Builds the CLI once per session. Set KADAKAS_SKIP_BUILD=1 to use an existing binary."""
    if not os.environ.get("KADAKAS_SKIP_BUILD"):
        subprocess.run(["cargo", "build", "-p", "kadakas-cli"], cwd=REPO, check=True)
    assert BIN.exists(), f"kadakas binary not found at {BIN}"
    yield
    # The private home of the child processes (helpers.isolated_env) is only for this session.
    shutil.rmtree(TEST_HOME, ignore_errors=True)


@pytest.fixture
def make_serve():
    """Factory for `kadakas serve [file]` processes. All are closed after the test."""
    started = []

    def make(*args) -> JsonRpc:
        client = JsonRpc(["serve", *ALLOW, *map(str, args)])
        started.append(client)
        return client

    yield make
    for client in started:
        client.close()


@pytest.fixture
def cad(make_serve) -> JsonRpc:
    """A fresh headless session: `kadakas serve` with an empty document."""
    return make_serve()


@pytest.fixture
def rpc(cad):
    """The rpc(method, params) helper of a fresh `kadakas serve` process."""
    return cad.rpc


@pytest.fixture
def mcp() -> McpClient:
    """A fresh `kadakas mcp` process, already initialized."""
    client = McpClient(["mcp", *ALLOW])
    client.initialize()
    yield client
    client.close()


@pytest.fixture
def mcp_raw():
    """A `kadakas mcp` process that has not been initialized yet, for handshake tests."""
    client = McpClient(["mcp", *ALLOW])
    yield client
    client.close()


@pytest.fixture
def mcp_flat() -> McpClient:
    """A fresh `kadakas mcp --mcp-flat` process (one tool per API method), already initialized."""
    client = McpClient(["mcp", "--mcp-flat", *ALLOW])
    client.initialize()
    yield client
    client.close()
