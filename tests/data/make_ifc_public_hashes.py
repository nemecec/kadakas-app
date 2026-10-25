"""Writes crates/kadakas-ifc/tests/fixtures/ifc_public_sha256.json (IOP-29).

The IFC corpus is NOT in the repository. The test helper crates/kadakas-ifc/tests/support/corpus.rs
downloads the files at run time from the commits below into tests/data/ifc-public/ (ignored by git) and
checks each against the SHA-256 in that JSON file. Run this script when the list or a commit changes:

    python3 -I tests/data/make_ifc_public_hashes.py

It needs the network and the standard library only. Licence of every file: Creative Commons Attribution
4.0 International (see docs/attribution.md). Files in Git LFS come from media.githubusercontent.com.
"""

import hashlib
import json
import sys
import urllib.parse
import urllib.request
from pathlib import Path

REPOS = {
    "bsi-cert": {
        "slug": "buildingSMART/Certification-datasets",
        "commit": "80d976a9b193a26a8e928c3e79bff67af1de68a8",
    },
    "bsi-community": {
        "slug": "buildingsmart-community/Community-Sample-Test-Files",
        "commit": "7ddf57a201f88a0c213d5322b02ed15e94a60a40",
    },
}

SCENES = [
    "Building-Architecture", "Building-Hvac", "Building-Landscaping", "Building-Structural",
    "Infra-Bridge", "Infra-Electrical", "Infra-Landscaping", "Infra-Plumbing", "Infra-Rail", "Infra-Road",
]
VERSIONS = ["IFC 2.3.0.1 (IFC 2x3 TC1)", "IFC 4.0.2.1 (IFC 4 ADD2 TC1)", "IFC 4.3.2.0 (IFC 4.3 ADD2)"]
REFERENCE_VIEW = [
    "basin-tessellation", "column-straight-rectangle-tessellation", "tessellated-item",
    "tessellation-with-individual-colors", "wall-with-opening-and-window",
]
ISO = "IFC 4.0.2.1 (IFC 4)/ISO Spec archive/"
ISO_FILES = [
    "basin-advanced-brep", "cube-advanced-brep", "bath-csg-solid", "csg-primitive", "beam-revolved-solid",
    "slab-openings", "grid-placement", "geographic-referencing", "wall-with-opening-and-window", "brep-model",
    "extruded-solid", "surface-model", "mapped-shape-with-transformation", "slab-standard-case",
    "wall-standard-case", "beam-extruded-solid", "tessellated-item", "basin-faceted-brep",
]
RC = "dev archive/IFC 4.3.RC Archive/"
TIN_FILES = [
    "Tin_1 (RC1)/UT_Tin_1.ifc", "Tin_2 (RC1)/UT_Tin_2.ifc", "Tin_3 (RC1)/UT_Tin_3.ifc", "Tin_4 (RC1)/UT_Tin_4.ifc",
    "Georeferencing_Tin_1 (RC1)/UT_Tin_in_MGA_56.ifc",
    "Georeferencing_Tin_2 (RC1)/UT_Tin_ifcMapConversion_to_place_in_MGA_56.ifc",
    "Georeferencing_Tin_3 (RC1)/UT_Tin_with_one_hole_in_MGA_56.ifc",
    "Georeferencing_Tin_4 (RC1)/UT_Tin_with_two colours_in_MGA2020_56.ifc",
    "Georeferencing_Tin_5 (RC1)/UT_Tin_ifcMapConversion_to_place_in_MGA_56_Standard_origin.ifc",
    "Georeferencing-1 (RC2)/UT_GeoRef_1.ifc",
    "Georeferencing_Tiny_Survey_1 (RC1)/UT_one_string_one_pont.ifc",
]
BENTLEY = "IFC 4.0.2.1 (IFC 4)/NURBS/Bentley Building Designer/"
BENTLEY_FILES = [
    "Cylinder/BsplineSurf.PolyCurve.ifc", "Cylinder/Facet.ifc", "Cylinder/OnlyBsplines.ifc",
    "Cylinder/Plane.Cyllinder.Ellipse.PolyCurve.ifc", "Cylinder/Plane.Cyllinder.PolyCurve.ifc",
    "Faces/5.Cone.ifc", "Faces/5.Cone.facet.ifc", "Faces/3.Torus.ifc", "Faces/3.Torus.facet.ifc",
    "Faces/6.RectWithHole.ifc", "Faces/6.RectWithHole.facet.ifc",
    "SolidsAndSheets/WithPlaneSurface.ifc", "SolidsAndSheets/WithPlaneSurface.facet.ifc",
]


def files():
    """(repo, path, lfs) of every corpus file."""
    out = []
    for v in VERSIONS:
        for s in SCENES:
            out.append(("bsi-cert", f"{v}/Simple-Scene/{s}.ifc", False))
    for n in REFERENCE_VIEW:
        out.append(("bsi-cert", f"{VERSIONS[1]}/ISO Spec - ReferenceView_V1.2/{n}.ifc", False))
    for n in ISO_FILES:
        out.append(("bsi-community", f"{ISO}{n}.ifc", True))
    # IFC XML and IFC JSON twins: not LFS, used by the tests of the refusal.
    out.append(("bsi-community", f"{ISO}geographic-referencing.xml", False))
    out.append(("bsi-community", f"{ISO}geographic-referencing.json", False))
    for n in TIN_FILES:
        out.append(("bsi-community", RC + n, True))
    for n in BENTLEY_FILES:
        out.append(("bsi-community", BENTLEY + n, True))
    return out


def url(repo, path, lfs):
    r = REPOS[repo]
    host = "media.githubusercontent.com/media" if lfs else "raw.githubusercontent.com"
    return f"https://{host}/{r['slug']}/{r['commit']}/{urllib.parse.quote(path)}"


def main():
    out = Path(__file__).resolve().parents[2] / "crates/kadakas-ifc/tests/fixtures/ifc_public_sha256.json"
    table = {}
    for repo, path, lfs in files():
        with urllib.request.urlopen(url(repo, path, lfs), timeout=120) as r:
            data = r.read()
        if data.startswith(b"version https://git-lfs"):
            sys.exit(f"{path}: a Git LFS pointer, not the file")
        table[f"{repo}:{path}"] = {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data), "lfs": lfs}
        print(f"{len(data):9d} {path}")
    doc = {"repos": REPOS, "files": dict(sorted(table.items()))}
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")


main()
