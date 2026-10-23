"""Reads an IFC file with IfcOpenShell and prints the volume and the faces of each product as JSON.
The behavior test runs it with `uv run --no-project --with ifcopenshell python -I ifc_volume_check.py <file>`."""

import json
import sys

import ifcopenshell
import ifcopenshell.geom
import ifcopenshell.util.shape as shape


def main(path):
    model = ifcopenshell.open(path)
    settings = ifcopenshell.geom.settings()
    out = []
    for p in model.by_type("IfcBuildingElementProxy"):
        geometry = ifcopenshell.geom.create_shape(settings, p).geometry
        reps = [r.RepresentationType for r in p.Representation.Representations]
        items = [i.is_a() for r in p.Representation.Representations for i in r.Items]
        out.append({
            "name": p.Name,
            "types": reps,
            "items": items,
            "triangles": len(geometry.faces) // 3,
            "volume": shape.get_volume(geometry),
            "area": shape.get_area(geometry),
        })
    print(json.dumps(out))


main(sys.argv[1])
