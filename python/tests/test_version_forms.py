"""The Python form of a Kadakas version (scripts/packager-version.sh pep440) must be a valid PEP 440
version and must sort like the semver order of the releases."""
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

packaging_version = pytest.importorskip("packaging.version")

REPO = Path(__file__).resolve().parents[2]
SCRIPT = "scripts/packager-version.sh"


def find_bash():
    """The bash that runs the script. On Windows it is Git bash, the shell that release.yml uses.
    `bash` on the PATH can be the one of WSL there, which does not read Windows paths and has no
    Python, so it is not taken."""
    if os.name != "nt":
        return shutil.which("bash")
    roots = []
    git = shutil.which("git")
    if git:
        roots.append(Path(git).resolve().parents[1])
    for var in ("ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"):
        if os.environ.get(var):
            roots.append(Path(os.environ[var]) / "Git")
    for root in roots:
        for sub in ("bin", "usr/bin"):
            candidate = root / sub / "bash.exe"
            if candidate.exists():
                return str(candidate)
    return None


BASH = find_bash()
# The script is used in release.yml on every system, so a missing bash is a failure, not a skip.
pytestmark = pytest.mark.skipif(BASH is None and os.name != "nt", reason="needs bash")

# # In the order of semver (kadakas_update::Version).
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
    if BASH is None:
        pytest.fail("Git bash was not found (looked next to git.exe and in Program Files\\Git). release.yml needs it.")
    # A relative path and the working directory avoid the path style of Windows. The script finds
    # Python through KADAKAS_PYTHON: python3 does not exist on Windows.
    env = dict(os.environ, KADAKAS_PYTHON=sys.executable)
    out = subprocess.run([BASH, SCRIPT, "pep440", version], cwd=REPO, env=env, capture_output=True, text=True)
    assert out.returncode == 0, "exit %s\nstdout: %s\nstderr: %s" % (out.returncode, out.stdout, out.stderr)
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
