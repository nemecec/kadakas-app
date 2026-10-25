"""Scenario: the cuts and the solids of IFC files from other programs (IOP-29). IfcOpenShell is the independent
reference: it writes the files, and it computes the volumes that Kadakas must get. Skipped when `uv` or
IfcOpenShell is not available."""

import json
import math
import shutil
import subprocess
from pathlib import Path

import pytest
from pytest import approx

from helpers import REPO
from test_ifc_import import mesh_volume, objects

HERE = Path(__file__).parent
MAKE = HERE / "ifc_make_solids.py"
VOLUMES = HERE / "ifc_volumes.py"


def uv_run(script, *args):
    if shutil.which("uv") is None:
        pytest.skip("uv is not installed")
    run = subprocess.run(
        ["uv", "run", "--no-project", "--with", "ifcopenshell", "python", "-I", str(script), *map(str, args)],
        capture_output=True, text=True, encoding="utf-8", timeout=600,
    )
    if run.returncode != 0 and "No solution found" in run.stderr + run.stdout:
        pytest.skip("IfcOpenShell is not available")
    assert run.returncode == 0, run.stderr
    return run.stdout


@pytest.fixture(scope="module")
def made(tmp_path_factory):
    out = tmp_path_factory.mktemp("ifc_solids")
    uv_run(MAKE, out)
    return out


def reference_volumes(path):
    return json.loads(uv_run(VOLUMES, path).splitlines()[-1])


def kadakas_volumes(cad, path):
    r = cad.result("import.file", {"path": str(path)})
    out = {}
    for i, o in enumerate(objects(cad)):
        out[o["data"]["ifc"].get("name") or f"{o['data']['ifc_class']}#{i}"] = mesh_volume(o["shape"])
    return r, out


@pytest.mark.parametrize("name, cut_openings", [("wall_door_window.ifc", 2), ("slab_hole_pocket.ifc", 2)])
def test_openings_are_cut_like_ifcopenshell_does(cad, made, name, cut_openings):
    ref = reference_volumes(made / name)
    r, got = kadakas_volumes(cad, made / name)
    assert len(got) == len(ref) == 1, (got, ref)
    ((n, v),) = got.items()
    assert v == approx(ref[n]["volume"], rel=0.002)
    assert any(f"{cut_openings} openings (IfcRelVoidsElement) were cut" in note for note in r["report"]["notes"])
    assert r["report"]["entities_read"] == r["report"]["entities_imported"] + r["report"]["entities_skipped"]


def test_a_wall_with_an_opening_from_the_hand_written_fixture(cad):
    r = cad.result("import.file", {"path": str(REPO / "tests" / "fixtures" / "ifc" / "wall_with_openings.ifc")})
    assert r["imported"] == 1
    volume = mesh_volume(objects(cad)[0]["shape"])
    assert volume == approx(4000 * 200 * 2800 - 1000 * 200 * 2100 - 800 * 200 * 1200, rel=1e-9)


def analytic(name):
    pi = math.pi
    return {
        "revolved_ring.ifc": {"Ring": pi * (1 / 3) * (300.0 ** 2 - 200.0 ** 2) * 50.0},
        "swept_pipe.ifc": {
            "Pipe": pi * 25.0 ** 2 * (1000.0 + math.hypot(800.0, 600.0) + math.hypot(0.0, 1000.0)),
            "Tube": pi * (30.0 ** 2 - 20.0 ** 2) * 1500.0,
        },
        "csg_solids.ifc": {
            "Block with hole": 2000.0 * 800.0 * 800.0 - pi * 200.0 ** 2 * 800.0,
            "Ball": 4.0 / 3.0 * pi * 300.0 ** 3,
            "Cone": pi * 200.0 ** 2 * 600.0 / 3.0,
        },
    }[name]


@pytest.mark.parametrize("name", ["revolved_ring.ifc", "swept_pipe.ifc", "csg_solids.ifc"])
def test_revolved_swept_and_csg_solids_have_the_volume_of_ifcopenshell(cad, made, name):
    ref = reference_volumes(made / name)
    r, got = kadakas_volumes(cad, made / name)
    assert set(got) == set(ref) == set(analytic(name))
    for n, v in got.items():
        # Both tessellate curves, so they differ a little from each other and from the formula.
        assert v == approx(analytic(name)[n], rel=0.04), (n, v)
        # IfcOpenShell is the second opinion. Its default kernel leaves the boolean of two CSG primitives
        # uncut (it gives the volume of the block), so it counts only where it agrees with the formula.
        # It also fails on some primitives ("error" instead of "volume").
        if ref[n].get("volume") == approx(analytic(name)[n], rel=0.04):
            assert v == approx(ref[n]["volume"], rel=0.04), (n, v, ref[n])
    assert r["report"]["skipped"] == [], r["report"]


CORPUS = REPO / "tests" / "data" / "ifc-public"


def corpus_file(repo, commit, path):
    f = CORPUS / repo / commit / path
    if not f.exists():
        pytest.skip(f"{f} is not in the cache: run `cargo test -p kadakas-ifc --test ifc_corpus` with a network first")
    return f


ISO = ("bsi-community", "7ddf57a2", "IFC 4.0.2.1 (IFC 4)/ISO Spec archive/")
REFERENCE_VIEW = ("bsi-cert", "80d976a9", "IFC 4.0.2.1 (IFC 4 ADD2 TC1)/ISO Spec - ReferenceView_V1.2/")


@pytest.mark.parametrize("name, twin", [
    ("cube-advanced-brep.ifc", None),
    # IfcOpenShell gives 2.7 times the volume of the basin from its advanced brep (a known problem of its
    # B-spline faces, see its issue 9570 for a similar one). The tessellated twin of the same basin in the
    # Reference View samples is the reference here.
    ("basin-advanced-brep.ifc", "basin-tessellation.ifc"),
    ("bath-csg-solid.ifc", None),
    ("beam-revolved-solid.ifc", None),
    ("csg-primitive.ifc", None),
    ("slab-openings.ifc", None),
])
def test_buildingsmart_iso_samples_have_the_volume_of_ifcopenshell(cad, name, twin):
    """The public ISO samples (CC BY 4.0, fetched by the Rust corpus test) against IfcOpenShell."""
    f = corpus_file(*ISO[:2], ISO[2] + name)
    ref = reference_volumes(corpus_file(*REFERENCE_VIEW[:2], REFERENCE_VIEW[2] + twin) if twin else f)
    r, got = kadakas_volumes(cad, f)
    # One product with a body; a slab also has its openings, which are not products of the reference.
    assert len(ref) == 1 and len(got) == 1, (ref, got)
    v = next(iter(got.values()))
    reference = next(iter(ref.values()))["volume"]
    assert v == approx(reference, rel=0.02), (name, v, reference)
