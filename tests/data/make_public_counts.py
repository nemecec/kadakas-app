"""Counts of the public sample files, read with the ezdxf reader (IOP-15).

The Rust test `dxf_corpus` compares the importer with these numbers: the entities of the model
space, the attributes of block references, the INSERTs, the entities of paper space and the names
of the layouts. The files are in tests/data/dxf-public/ (fetched by the test, ignored by git).

Run: uv run --with ezdxf python -I tests/data/make_public_counts.py
"""

import json
import sys
from pathlib import Path

import ezdxf
from ezdxf import recover

ROOT = Path(__file__).resolve().parent / "dxf-public"
SUMS = Path(__file__).resolve().parents[2] / "crates/kadakas-io/tests/fixtures/dxf_public_sha256.json"
OUT = Path(__file__).resolve().parents[2] / "crates/kadakas-io/tests/fixtures/dxf_public_counts.json"


def read(path):
    try:
        return ezdxf.readfile(str(path))
    except Exception:
        try:
            doc, _ = recover.readfile(str(path))
            return doc
        except Exception:
            return None


def main():
    counts = {}
    for path in sorted(ROOT.glob("*/*/**/*")):
        if path.suffix.lower() != ".dxf" or not path.is_file():
            continue
        repo = path.relative_to(ROOT).parts[0]
        rel = "/".join(path.relative_to(ROOT).parts[2:])
        key = f"{repo}:{rel}"
        doc = read(path)
        if doc is None:
            counts[key] = None
            continue
        msp = list(doc.modelspace())
        paper = 0
        layouts = []
        for layout in doc.layouts:
            if layout.name == "Model":
                continue
            paper += len(list(layout))
            layouts.append(layout.name)
        counts[key] = {
            "version": doc.dxfversion,
            "model": len(msp),
            "attribs": sum(len(e.attribs) for e in msp if e.dxftype() == "INSERT"),
            "inserts": sum(1 for e in msp if e.dxftype() == "INSERT"),
            "paper": paper,
            "layouts": layouts,
            "viewports": sum(1 for lay in doc.layouts if lay.name != "Model" for e in lay if e.dxftype() == "VIEWPORT"),
            # The plain text of every MTEXT of the model space, as the ezdxf reader finds it.
            "mtext": [e.plain_text(split=False) for e in msp if e.dxftype() == "MTEXT"],
            # Points on the splines of the model space (ezdxf flattens them within 0.01 units).
            "splines": [
                [[round(p.x, 4), round(p.y, 4)] for p in e.construction_tool().flattening(0.01)]
                for e in msp
                if e.dxftype() == "SPLINE" and "spline" in rel
            ],
        }
    # The SHA-256 of every file, for the check of the tests.
    import hashlib

    sums = {}
    for path in sorted(ROOT.glob("*/*/**/*")):
        if path.suffix.lower() == ".dxf" and path.is_file():
            repo = path.relative_to(ROOT).parts[0]
            rel = "/".join(path.relative_to(ROOT).parts[2:])
            sums[f"{repo}:{rel}"] = hashlib.sha256(path.read_bytes()).hexdigest()
    SUMS.write_text(json.dumps(sums, indent=1, sort_keys=True) + "\n")
    OUT.write_text(json.dumps(counts, indent=1, sort_keys=True) + "\n")
    print(f"{len(counts)} files, {sum(1 for v in counts.values() if v is None)} unreadable", file=sys.stderr)


main()
