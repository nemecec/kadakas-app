"""The Python form of a Kadakas version (scripts/packager-version.sh pep440) must be a valid PEP 440
version and must sort like the semver order of the releases."""
import shutil
import subprocess
from pathlib import Path

import pytest

packaging_version = pytest.importorskip("packaging.version")

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts" / "packager-version.sh"
pytestmark = pytest.mark.skipif(shutil.which("bash") is None, reason="needs bash")

# In the order of semver (kadakas_update::Version).
ORDER = [
    "0.1.0-alpha.1",
    "0.1.0-beta.1",
    "0.1.0-beta.2",
    "0.1.0-dev",
    "0.1.0-rc.1",
    "0.1.0",
    "0.1.1-dev",
    "0.1.1",
]


def pep440(version):
    out = subprocess.run(["bash", str(SCRIPT), "pep440", version], capture_output=True, text=True, check=True)
    return out.stdout.strip()


def test_every_form_is_valid_and_sorts_like_semver():
    forms = [packaging_version.Version(pep440(v)) for v in ORDER]
    assert forms == sorted(forms)
    assert len(set(forms)) == len(forms)


def test_the_development_form_sorts_between_the_betas_and_the_release():
    dev = packaging_version.Version(pep440("0.1.0-dev"))
    assert packaging_version.Version(pep440("0.1.0-beta.9")) < dev < packaging_version.Version("0.1.0")
    assert dev < packaging_version.Version(pep440("0.1.0-rc.1"))


def test_the_package_version_is_the_workspace_version_in_python_form():
    import kadakas

    text = (REPO / "Cargo.toml").read_text()
    ws = next(line.split('"')[1] for line in text.splitlines() if line.startswith("version = "))
    assert kadakas.__version__ == pep440(ws)
