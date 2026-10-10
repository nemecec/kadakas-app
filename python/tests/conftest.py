"""Fixtures: each test gets its own ``cadektor serve`` process, so tests never share state.

Set CADEKTOR_BIN to the program (default: ``target/debug/cadektor`` of the repository)."""

from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
PYTHON_DIR = HERE.parent
REPO = PYTHON_DIR.parent
sys.path.insert(0, str(PYTHON_DIR))

BIN = Path(os.environ.get("CADEKTOR_BIN", REPO / "target" / "debug" / "cadektor"))
os.environ["CADEKTOR_BIN"] = str(BIN)

import cadektor  # noqa: E402


@pytest.fixture(scope="session", autouse=True)
def binary() -> Path:
    if not BIN.exists():
        pytest.exit(f"cadektor program not found at {BIN}. Build it or set CADEKTOR_BIN", returncode=2)
    return BIN


@pytest.fixture
def doc(tmp_path: Path):
    """A fresh headless session. It may read and write the temp folder."""
    allowed = [Path(tempfile.gettempdir()).resolve(), REPO]
    d = cadektor.open(allow_dir=allowed)
    yield d
    d.close()


@pytest.fixture
def open_doc(tmp_path: Path):
    """A session whose host lets agents run their own scripts."""
    d = cadektor.open(allow_dir=[Path(tempfile.gettempdir()).resolve()], allow_agent_scripts=True)
    yield d
    d.close()
