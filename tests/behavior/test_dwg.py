"""Scenario: a surveyor sends a DWG file. Cadektor does not read DWG itself: it runs a converter that the user
installed (ODA File Converter or LibreDWG). The tests put stub scripts on PATH that stand for the converter."""

import os
import stat
import sys
from pathlib import Path

import pytest

DXF = """0
SECTION
2
ENTITIES
0
LINE
8
Walls
10
0.0
20
0.0
30
0.0
11
2500.0
21
0.0
31
0.0
0
ENDSEC
0
EOF
"""

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="the stub converters are shell scripts")


def script(folder: Path, name: str, body: str):
    p = folder / name
    p.write_text("#!/bin/sh\n" + body + "\n")
    p.chmod(p.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


@pytest.fixture
def libredwg(tmp_path, monkeypatch):
    """dwg2dxf and dxf2dwg stubs first on PATH."""
    stubs = tmp_path / "stubs"
    stubs.mkdir()
    script(stubs, "dwg2dxf", f'[ "$1" = "-y" ] && [ "$2" = "-o" ] || exit 2\ngrep -q FAKEDWG "$4" || {{ echo "not a dwg file" >&2; exit 3; }}\ncat > "$3" <<\'STUB_END\'\n{DXF}STUB_END')
    script(stubs, "dxf2dwg", '[ "$1" = "-y" ] && [ "$2" = "-o" ] || exit 2\ngrep -q ENTITIES "$4" || exit 3\necho FAKEDWG > "$3"')
    monkeypatch.setenv("PATH", f"{stubs}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.delenv("CADEKTOR_DWG_CONVERTER", raising=False)
    return stubs


def test_import_runs_the_converter_and_the_dxf_importer(make_serve, libredwg, tmp_path):
    cad = make_serve()
    dwg = tmp_path / "survey.dwg"
    dwg.write_bytes(b"FAKEDWG\n")
    r = cad.result("import.file", {"path": str(dwg)})
    assert r["imported"] == 1
    assert r["report"]["format"] == "dwg"
    assert any("LibreDWG" in n for n in r["report"]["notes"])
    shape = next(iter(cad.doc()["objects"].values()))["shape"] if isinstance(cad.doc()["objects"], dict) else cad.doc()["objects"][0]["shape"]
    assert shape["kind"] == "line"
    # One undo step.
    cad.result("undo")
    assert cad.result("document.summary")["object_count"] == 0


def test_export_writes_dxf_and_converts_it(make_serve, libredwg, tmp_path):
    cad = make_serve()
    cad.execute({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [100, 0]}})
    out = tmp_path / "out.dwg"
    r = cad.result("export.file", {"path": str(out), "format": "dwg"})
    assert r["exported"] == 1
    assert out.read_text().strip() == "FAKEDWG"
    # A second export needs overwrite in a restricted session.
    err = cad.error("export.file", {"path": str(out), "format": "dwg"})
    assert "overwrite" in err["message"]
    cad.result("export.file", {"path": str(out), "format": "dwg", "overwrite": True})


def test_converter_errors_are_reported(make_serve, libredwg, tmp_path):
    cad = make_serve()
    bad = tmp_path / "bad.dwg"
    bad.write_bytes(b"this is not a drawing")
    err = cad.error("import.file", {"path": str(bad)})
    assert "not a dwg file" in err["message"]
    # Options are checked.
    ok = tmp_path / "ok.dwg"
    ok.write_bytes(b"FAKEDWG")
    err = cad.error("import.file", {"path": str(ok), "options": {"converter_timeout_s": 0}})
    assert "converter_timeout_s" in err["message"]


def test_a_hanging_converter_is_stopped(make_serve, libredwg, tmp_path):
    script(libredwg, "dwg2dxf", "sleep 60")
    cad = make_serve()
    dwg = tmp_path / "slow.dwg"
    dwg.write_bytes(b"FAKEDWG")
    err = cad.error("import.file", {"path": str(dwg), "options": {"converter_timeout_s": 1}})
    assert "stopped" in err["message"]


def test_without_a_converter_the_error_says_how_to_fix_it(make_serve, tmp_path, monkeypatch):
    if Path("/Applications/ODAFileConverter.app").exists() or Path("/usr/bin/ODAFileConverter").exists():
        pytest.skip("an ODA File Converter is installed on this computer")
    empty = tmp_path / "empty"
    empty.mkdir()
    # Keep the system tools but drop folders that hold a converter.
    keep = [p for p in os.environ["PATH"].split(os.pathsep) if not any((Path(p) / n).exists() for n in ("dwg2dxf", "ODAFileConverter"))]
    monkeypatch.setenv("PATH", os.pathsep.join([str(empty)] + keep))
    monkeypatch.delenv("CADEKTOR_DWG_CONVERTER", raising=False)
    cad = make_serve()
    dwg = tmp_path / "none.dwg"
    dwg.write_bytes(b"FAKEDWG")
    err = cad.error("import.file", {"path": str(dwg)})
    assert "no DWG converter" in err["message"] and "docs/dwg.md" in err["message"] and "DXF" in err["message"]
    err = cad.error("export.file", {"path": str(tmp_path / "x.dwg")})
    assert "docs/dwg.md" in err["message"]
    d = cad.result("describe")["io"]["dwg"]
    assert d["converter"] is None and "setup" in d


def test_describe_names_the_converter(make_serve, libredwg):
    cad = make_serve()
    d = cad.result("describe")["io"]["dwg"]
    assert d["converter"]["name"] == "LibreDWG" and d["converter"]["can_write"] is True
    assert any("dwg" in f for f in cad.result("describe")["io"]["import_formats"])


def test_the_path_must_be_allowed_and_end_in_dwg(make_serve, libredwg, tmp_path):
    cad = make_serve()
    wrong = tmp_path / "survey.txt"
    wrong.write_bytes(b"FAKEDWG")
    err = cad.error("import.file", {"path": str(wrong), "format": "dwg"})
    assert err["message"]
