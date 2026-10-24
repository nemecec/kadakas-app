"""Facts about public sample files for the tests of IOP-27, read with the ezdxf reader.

The Rust tests (`crates/kadakas-io/tests/dxf_leftovers.rs`) compare the importer with these numbers:
the pattern hatches of the model space (name, pattern lines, boundary paths). The files are in
tests/data/dxf-public/ (fetched by the tests, ignored by git).

Run: uv run --with ezdxf --with shapely python -I tests/data/make_iop27_facts.py
"""

import json
from pathlib import Path

import ezdxf
from ezdxf import path as ezpath
from ezdxf.xclip import XClip
from shapely.geometry import LineString, Polygon

ROOT = Path(__file__).resolve().parent / "dxf-public"
OUT = Path(__file__).resolve().parents[2] / "crates/kadakas-io/tests/fixtures/dxf_iop27.json"

PATTERN_FILES = [
    "ezdxf/7b202613/examples_dxf/transform_hatch_pattern.dxf",
    "ezdxf/7b202613/examples_dxf/hatch_issues.dxf",
    "gdal/739fcf03/autotest/ogr/data/dxf/hatch_pattern_generated_by_gdal.dxf",
]


def pattern_hatches(doc):
    out = []
    for e in doc.modelspace().query("HATCH"):
        if e.dxf.solid_fill:
            continue
        lines = []
        for line in e.pattern.lines if e.pattern else []:
            lines.append(
                [
                    float(line.angle),
                    float(line.base_point[0]),
                    float(line.base_point[1]),
                    float(line.offset[0]),
                    float(line.offset[1]),
                    [float(d) for d in line.dash_length_items],
                ]
            )
        out.append(
            {
                "name": e.dxf.pattern_name,
                "paths": len(e.paths),
                "extrusion_z": float(e.dxf.extrusion[2]),
                "lines": lines,
            }
        )
    return out


XCLIP_FILE = "ezdxf/7b202613/examples_dxf/xclip.dxf"


def xclip_facts(doc):
    """The length of the geometry of each block reference after the clip, found with shapely: the
    entities of the reference in the drawing (`virtual_entities`) cut by the polygon of the clip in
    the drawing (inverted clips: the polygon without the part that the clip cuts away)."""
    inserts = []
    for ins in doc.modelspace().query("INSERT"):
        clip = XClip(ins)
        wcs = clip.get_wcs_clipping_path()
        if clip.is_clipping_enabled and wcs.vertices:
            region = Polygon([(v.x, v.y) for v in wcs.vertices]).buffer(0)
            if clip.is_inverted_clip and wcs.inverted_clip:
                region = region.difference(Polygon([(v.x, v.y) for v in wcs.inverted_clip]).buffer(0))
        else:
            region = None
        length = 0.0
        for e in ins.virtual_entities():
            p = ezpath.make_path(e)
            for sub in p.sub_paths():
                pts = [(v.x, v.y) for v in sub.flattening(0.0005)]
                if len(pts) < 2:
                    continue
                line = LineString(pts)
                length += (line if region is None else line.intersection(region)).length
        inserts.append({"clipped": region is not None, "inverted": bool(clip.is_inverted_clip), "length": length})
    return {"inserts": inserts, "total_length": sum(i["length"] for i in inserts)}


def main():
    facts = {"patterns": {}}
    for rel in PATTERN_FILES:
        doc = ezdxf.readfile(str(ROOT / rel))
        key = rel.split("/")[0] + ":" + "/".join(rel.split("/")[2:])
        facts["patterns"][key] = {"insunits": doc.header.get("$INSUNITS", 0), "inserts": sum(1 for e in doc.modelspace() if e.dxftype() == "INSERT"), "hatches": pattern_hatches(doc)}
    facts["xclip"] = xclip_facts(ezdxf.readfile(str(ROOT / XCLIP_FILE)))
    OUT.write_text(json.dumps(facts, indent=1, sort_keys=True) + "\n")


main()
