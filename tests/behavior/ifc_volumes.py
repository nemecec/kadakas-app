"""Prints the volume that IfcOpenShell computes for each product of an IFC file, as JSON:
`{"<name>": {"class": "IfcWall", "volume": 1.0e9}}` (a product without a name is `<class>#<index>`) (cubic millimeters).
IfcOpenShell cuts the openings (IfcRelVoidsElement) by default. The behavior tests run it with
`uv run --no-project --with ifcopenshell python -I ifc_volumes.py <file>`."""

import json
import sys

import ifcopenshell
import ifcopenshell.geom
import ifcopenshell.util.shape as shape


def main(path):
    model = ifcopenshell.open(path)
    settings = ifcopenshell.geom.settings()
    # IfcOpenShell works in meters: the volume is in cubic meters. The answer is in cubic millimeters.
    out = {}
    for index, p in enumerate(model.by_type("IfcProduct")):
        if p.Representation is None or p.is_a("IfcOpeningElement"):
            continue
        try:
            geometry = ifcopenshell.geom.create_shape(settings, p).geometry
        except RuntimeError as err:
            out[p.Name or f"{p.is_a()}#{index}"] = {"class": p.is_a(), "error": str(err)}
            continue
        out[p.Name or f"{p.is_a()}#{index}"] = {"class": p.is_a(), "volume": shape.get_volume(geometry) * 1e9}
    print(json.dumps(out))


main(sys.argv[1])
