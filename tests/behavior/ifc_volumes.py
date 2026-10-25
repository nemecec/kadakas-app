"""Prints the volume that IfcOpenShell computes for each product of an IFC file, as JSON:
`{"<name>": {"class": "IfcWall", "volume": 1.0e9}}` (mm cubed when the file is in millimeters).
IfcOpenShell cuts the openings (IfcRelVoidsElement) by default. The behavior tests run it with
`uv run --no-project --with ifcopenshell python -I ifc_volumes.py <file>`."""

import json
import sys

import ifcopenshell
import ifcopenshell.geom
import ifcopenshell.util.shape as shape
import ifcopenshell.util.unit as unit


def main(path):
    model = ifcopenshell.open(path)
    settings = ifcopenshell.geom.settings()
    # IfcOpenShell works in meters; the factor takes the volume back to the cubed length unit of the file.
    scale = unit.calculate_unit_scale(model)
    out = {}
    for p in model.by_type("IfcProduct"):
        if p.Representation is None or p.is_a("IfcOpeningElement"):
            continue
        try:
            geometry = ifcopenshell.geom.create_shape(settings, p).geometry
        except RuntimeError as err:
            out[p.Name] = {"class": p.is_a(), "error": str(err)}
            continue
        out[p.Name] = {"class": p.is_a(), "volume": shape.get_volume(geometry) / scale ** 3}
    print(json.dumps(out))


main(sys.argv[1])
